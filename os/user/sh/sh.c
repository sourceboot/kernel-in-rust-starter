/*
 * user/sh/sh.c -- lab 23's shell, the course's (RATIFY Q6): the learner's kernel runs it,
 * the learner does not write it. The finish line of the title.
 *
 *     sh                          reads commands from fd 0, one line at a time
 *
 * The loop: print the prompt "$ " on fd 1; read fd 0 ONE BYTE per read call up to '\n' (so
 * a command that reads fd 0 gets the bytes after its own line, never ones the shell took);
 * run the line; repeat. A read of 0 (EOF) with nothing typed ends the shell with the last
 * status; a partial line before EOF is run first. A refused read prints
 * `sh: <stdin>: <errno>` and exits 1.
 *
 * A line is words separated by spaces or tabs, and three operators that need no spaces
 * around them:
 *     cmd > path     fd 1 to path, opened O_WRONLY|O_CREAT|O_TRUNC, mode 0644
 *     cmd < path     fd 0 from path, opened O_RDONLY
 *     a | b | c      a pipeline of up to 8 commands (pipe2 with O_CLOEXEC)
 * "..." quotes spaces and the operators; there is no other quoting, no globbing, no job
 * control, no variables you can set. Inside or outside quotes, `$?` is the last status and
 * `$NAME` is NAME's value in the shell's own environment (empty if unset); any other `$` is
 * itself. The status of a pipeline is its last command's: the exit code, or 128 + the signal
 * for a process the kernel killed (CONTRACT §A.6).
 *
 * Built in: `cd [dir]` (no dir: "/"), `exit [n]` (no n: the last status), `pwd`, and
 * `echo [word...]` (the words joined by one space, then '\n'; no options). `cd` and `exit`
 * alone on a line run in the shell itself and open no redirection file; everywhere else, and
 * `echo` and `pwd` always, a builtin runs in a forked child, which takes the redirections and
 * pipes like any command. Any other name with no '/' is run as /bin/<name>; a name with a '/'
 * is run as given (relative to the cwd). Children get the shell's envp unchanged.
 *
 * Messages, each one line on fd 2 (status in brackets):
 *     sh: <name>: ENOENT            no such command (127; another execve refusal: 126)
 *     sh: <path>: <errno>           a redirection's open refused (1; the command does not run)
 *     sh: syntax: <token>           <token> is |, <, >, newline or " (2; nothing runs)
 *     sh: line too long             more than 1023 bytes typed, or 2047 after $ expansion (2)
 *     sh: too many words            more than 63 words in one command (2)
 *     sh: too many commands         more than 8 commands in one pipeline (2)
 *     sh: fork: <errno>, sh: pipe: <errno>    (1; commands already started are waited for)
 *     sh: cd: <dir>: <errno>  (1)   sh: pwd: <errno>  (1)   sh: exit: <arg>: EINVAL  (2, stays)
 * <errno> is the name (ENOENT, ...) or -N. Syscalls: read, write, open, close, pipe2, dup2,
 * fork, execve, wait4, chdir, getcwd, exit -- each in CONTRACT §A.4 at or before lab 23.
 */
#include "../sys.h"

#define LINE_BYTES 1024
#define WORD_BYTES 2048
#define MAX_WORDS  64
#define MAX_STAGES 8

struct stage { int argc; char *argv[MAX_WORDS]; char *in, *out; };

static char text[LINE_BYTES];          /* the line as typed */
static char words[WORD_BYTES];         /* every word, expanded, NUL-terminated */
static struct stage stages[MAX_STAGES];
static int nstages;
static int last;                       /* $? */
static char **env;

/* ---- reading a line ---- */
/* 1: a line is in text; 0: EOF before any byte; 2: an overlong line was dropped. */
static int read_line(void)
{
    usize n = 0;
    int over = 0;
    for (;;) {
        char c;
        i64 r = sys_read(STDIN_FILENO, &c, 1);
        if (r < 0) { err_line("sh", "<stdin>", r); sys_exit(1); }
        if (r == 0) {
            if (n == 0 && !over) return 0;
            break;
        }
        if (c == '\n') break;
        if (n < LINE_BYTES - 1) text[n++] = c; else over = 1;
    }
    text[n] = 0;
    return over ? 2 : 1;
}

/* ---- parsing: text -> stages ---- */
static usize wn;                       /* bytes used in words[] */
static int put(char c)
{
    if (wn == WORD_BYTES) return 0;
    words[wn++] = c;
    return 1;
}
static int put_str(const char *s) { while (*s) if (!put(*s++)) return 0; return 1; }

static const char *getenv_(const char *name, usize len)
{
    for (char **e = env; e && *e; e++) {
        usize i = 0;
        while (i < len && (*e)[i] == name[i]) i++;
        if (i == len && (*e)[i] == '=') return *e + len + 1;
    }
    return "";
}
static int is_name(char c, int first)
{
    return c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (!first && c >= '0' && c <= '9');
}

/* Returns 0 and fills stages[], or prints the message and returns the status (2). */
static int parse(void)
{
    nstages = 1;
    stages[0] = (struct stage){ 0, {0}, 0, 0 };
    wn = 0;
    const char *s = text;
    char **target = 0;                 /* the redirect waiting for its word */
    for (;;) {
        while (*s == ' ' || *s == '\t' || *s == '\r') s++;
        struct stage *cur = &stages[nstages - 1];
        if (*s == 0 || *s == '|' || *s == '<' || *s == '>') {
            /* an operator with no word for it: a redirect's missing path, an empty command */
            if (target || (*s == '|' && cur->argc == 0) || (*s == 0 && cur->argc == 0 && (nstages > 1 || cur->in || cur->out))) {
                err_msg("sh", *s == 0 ? "syntax: newline" : *s == '|' ? "syntax: |" : *s == '<' ? "syntax: <" : "syntax: >");
                return 2;
            }
            if (*s == 0) return 0;
            if (*s == '|') {
                if (nstages == MAX_STAGES) { err_msg("sh", "too many commands"); return 2; }
                stages[nstages++] = (struct stage){ 0, {0}, 0, 0 };
            } else target = *s == '<' ? &cur->in : &cur->out;
            s++;
            continue;
        }
        /* a word: runs to an unquoted space or operator */
        char *w = words + wn;
        int quoted = 0;
        while (*s && (quoted || !(*s == ' ' || *s == '\t' || *s == '\r' || *s == '|' || *s == '<' || *s == '>'))) {
            if (*s == '"') { quoted = !quoted; s++; continue; }
            if (*s == '$' && s[1] == '?') {
                char t[12];
                int i = 12;
                u64 v = (u64)(u32)last;
                do { t[--i] = (char)('0' + v % 10); v /= 10; } while (v);
                while (i < 12) if (!put(t[i++])) goto long_line;
                s += 2;
                continue;
            }
            if (*s == '$' && is_name(s[1], 1)) {
                const char *name = ++s;
                while (is_name(*s, 0)) s++;
                if (!put_str(getenv_(name, (usize)(s - name)))) goto long_line;
                continue;
            }
            if (!put(*s++)) goto long_line;
        }
        if (quoted) { err_msg("sh", "syntax: \""); return 2; }
        if (!put(0)) goto long_line;
        if (target) { *target = w; target = 0; continue; }
        if (cur->argc == MAX_WORDS - 1) { err_msg("sh", "too many words"); return 2; }
        cur->argv[cur->argc++] = w;
    }
long_line:
    err_msg("sh", "line too long");
    return 2;
}

/* ---- running ---- */
static int builtin(const char *name)
{
    return streq(name, "cd") || streq(name, "exit") || streq(name, "pwd") || streq(name, "echo");
}

/* Runs a builtin; returns its status. `exit` does not return. */
static int run_builtin(struct stage *st)
{
    char **a = st->argv;
    if (streq(a[0], "cd")) {
        const char *dir = st->argc > 1 ? a[1] : "/";
        i64 r = sys_chdir(dir);
        if (r < 0) { err_line("sh: cd", dir, r); return 1; }
        return 0;
    }
    if (streq(a[0], "exit")) {
        u64 v = (u64)(u32)last;
        if (st->argc > 1 && !dec_parse(a[1], &v)) { err_line("sh: exit", a[1], -EINVAL); return 2; }
        sys_exit((int)(v & 0xff));
    }
    if (streq(a[0], "pwd")) {
        char cwd[1024];
        i64 r = sys_getcwd(cwd, sizeof cwd);
        if (r < 0) { err_line("sh", "pwd", r); return 1; }
        usize n = strlen(cwd);
        cwd[n] = '\n';
        write_buf(cwd, n + 1);
        return 0;
    }
    /* echo */
    static struct obuf out;
    out.fd = STDOUT_FILENO;
    for (int i = 1; i < st->argc; i++) {
        if (i > 1) ob_putc(&out, ' ');
        ob_str(&out, a[i]);
    }
    ob_putc(&out, '\n');
    ob_flush(&out);
    return out.err ? 1 : 0;
}

/* In the child: wire fds 0/1, apply the redirections, then exec or run the builtin. */
static __attribute__((noreturn)) void child(struct stage *st, int in, int out)
{
    if (in >= 0) sys_dup2(in, STDIN_FILENO);
    if (out >= 0) sys_dup2(out, STDOUT_FILENO);
    if (st->in) {
        i64 fd = sys_open(st->in, O_RDONLY | O_CLOEXEC, 0);
        if (fd < 0) { err_line("sh", st->in, fd); sys_exit(1); }
        sys_dup2((int)fd, STDIN_FILENO);
    }
    if (st->out) {
        i64 fd = sys_open(st->out, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
        if (fd < 0) { err_line("sh", st->out, fd); sys_exit(1); }
        sys_dup2((int)fd, STDOUT_FILENO);
    }
    st->argv[st->argc] = 0;
    if (builtin(st->argv[0])) sys_exit(run_builtin(st));
    char path[LINE_BYTES + 8];
    const char *p = st->argv[0];
    int slash = 0;
    for (const char *c = p; *c; c++) if (*c == '/') slash = 1;
    if (!slash) {
        usize n = 0;
        for (const char *c = "/bin/"; *c; c++) path[n++] = *c;
        for (const char *c = p; *c && n < sizeof path - 1; c++) path[n++] = *c;
        path[n] = 0;
        p = path;
    }
    i64 r = sys_execve(p, st->argv, env);
    err_line("sh", st->argv[0], r);
    sys_exit(r == -ENOENT ? 127 : 126);
}

static int run(void)
{
    if (nstages == 1 && (streq(stages[0].argv[0], "cd") || streq(stages[0].argv[0], "exit"))) {
        stages[0].argv[stages[0].argc] = 0;
        return run_builtin(&stages[0]);
    }
    i64 pids[MAX_STAGES];
    int started = 0, prev = -1, status = 0;
    for (int i = 0; i < nstages; i++) {
        int p[2] = { -1, -1 };
        if (i < nstages - 1) {
            i64 r = sys_pipe2(p, O_CLOEXEC);
            if (r < 0) { err_line("sh", "pipe", r); status = 1; break; }
        }
        i64 pid = sys_fork();
        if (pid == 0) child(&stages[i], prev, p[1]);
        if (prev >= 0) sys_close(prev);
        if (p[1] >= 0) sys_close(p[1]);
        prev = p[0];
        if (pid < 0) { err_line("sh", "fork", pid); status = 1; break; }
        pids[started++] = pid;
    }
    if (prev >= 0) sys_close(prev);
    for (int i = 0; i < started; i++) {
        int ws = 0;
        i64 r = sys_wait4(pids[i], &ws, 0);
        if (i == started - 1 && status == 0)
            status = r < 0 ? 1 : WIFEXITED(ws) ? WEXITSTATUS(ws) : 128 + WTERMSIG(ws);
    }
    return status;
}

int main(int argc, char **argv, char **envp)
{
    (void)argc; (void)argv;
    env = envp;
    for (;;) {
        write_str("$ ");
        int r = read_line();
        if (r == 0) return last;
        if (r == 2) { err_msg("sh", "line too long"); last = 2; continue; }
        int p = parse();
        if (p) { last = p; continue; }
        if (stages[0].argc == 0) continue;    /* an empty line keeps $? */
        last = run();
    }
}
