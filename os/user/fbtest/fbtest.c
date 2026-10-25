/* fbtest -- lab 26: the framebuffer the way a Linux program finds it.
 *
 *   fbtest fill      map, fill, unmap: no fork
 *   fbtest           the same, and a forked child draws
 *
 * Opens /dev/fb0 read-write and asks it for its geometry with the two
 * ioctls doomgeneric's linuxvt backend uses (linux/fb.h):
 * FBIOGET_VSCREENINFO and FBIOGET_FSCREENINFO. Prints one line for each,
 *
 *   var <xres> <yres> <xres_virtual> <yres_virtual> <bits_per_pixel> r <off>/<len> g <off>/<len> b <off>/<len> t <off>/<len>
 *   fix <id> smem <smem_len> line <line_length> type <type> visual <visual> other <errno of FBIOPAN_DISPLAY>
 *
 * then maps yres * line_length bytes of it MAP_SHARED (and tries the same
 * MAP_PRIVATE, printing what that returned) and draws a fixed picture, its
 * colours built from the bitfields the first ioctl gave:
 *
 *   the whole screen   (0x18, 0x2c, 0x48)
 *   a rectangle        (0xe8, 0x70, 0x2c), from 3/4 of the width and height,
 *                      a fifth of each wide and high -- drawn by a CHILD
 *                      after fork, through the mapping it inherited.
 *
 * `fbtest fill` stops after the fill: it reads the pixel in the
 * rectangle's middle back through its mapping and prints
 *
 *   fill <the pixel, 8 hex digits>
 *
 * `fbtest` forks; once the child has drawn, the parent reads one pixel of
 * the rectangle back through its own mapping and prints
 *
 *   private <0 or -errno> child <the pixel, 8 hex digits>
 *
 * Then, with the mapping still in place (and the child's too, after a
 * fork), it reads fd 0 until end of file: whoever runs it looks at the
 * screen meanwhile. Then the parent reaps the child, unmaps, and prints
 *
 *   munmap <0 or -errno>
 *
 * No nonce: the picture is what the rubric's fb row samples, from outside.
 * Any failure prints `fbtest: <what>` and exits 1.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/fb.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static struct fb_var_screeninfo var;
static struct fb_fix_screeninfo fix;

static int fail(const char *what) {
    printf("fbtest: %s: %s\n", what, strerror(errno));
    return 1;
}

/* One 8-bit channel value at its bitfield. */
static uint32_t chan(uint32_t v, const struct fb_bitfield *f) {
    return f->length >= 8 ? (v << (f->length - 8)) << f->offset : (v >> (8 - f->length)) << f->offset;
}

static uint32_t rgb(uint32_t r, uint32_t g, uint32_t b) {
    return chan(r, &var.red) | chan(g, &var.green) | chan(b, &var.blue);
}

/* Until fd 0 is at end of file (or is no file). */
static void hold(void) {
    char c;
    while (read(0, &c, 1) > 0) {
    }
}

static void fill(uint8_t *fb, uint32_t x0, uint32_t y0, uint32_t w, uint32_t h, uint32_t px) {
    for (uint32_t y = y0; y < y0 + h; y++) {
        uint32_t *row = (uint32_t *)(fb + (size_t)y * fix.line_length);
        for (uint32_t x = x0; x < x0 + w; x++) row[x] = px;
    }
}

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    int fill_only = argc > 1 && strcmp(argv[1], "fill") == 0;
    int fd = open("/dev/fb0", O_RDWR);
    if (fd < 0) return fail("open /dev/fb0");
    if (ioctl(fd, FBIOGET_VSCREENINFO, &var) != 0) return fail("FBIOGET_VSCREENINFO");
    printf("var %u %u %u %u %u r %u/%u g %u/%u b %u/%u t %u/%u\n", var.xres, var.yres, var.xres_virtual, var.yres_virtual,
           var.bits_per_pixel, var.red.offset, var.red.length, var.green.offset, var.green.length, var.blue.offset,
           var.blue.length, var.transp.offset, var.transp.length);
    if (ioctl(fd, FBIOGET_FSCREENINFO, &fix) != 0) return fail("FBIOGET_FSCREENINFO");
    int other = ioctl(fd, FBIOPAN_DISPLAY, &var) == 0 ? 0 : -errno;
    printf("fix %.16s smem %u line %u type %u visual %u other %d\n", fix.id, fix.smem_len, fix.line_length, fix.type, fix.visual,
           other);
    if (var.bits_per_pixel != 32) {
        errno = EINVAL;
        return fail("not 32 bits a pixel");
    }
    size_t len = (size_t)var.yres * fix.line_length;
    void *priv = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_PRIVATE, fd, 0);
    int private = priv == MAP_FAILED ? -errno : 0;
    if (priv != MAP_FAILED) munmap(priv, len);
    uint8_t *fb = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (fb == MAP_FAILED) return fail("mmap MAP_SHARED");
    fill(fb, 0, 0, var.xres, var.yres, rgb(0x18, 0x2c, 0x48));
    uint32_t x0 = var.xres / 4 * 3, y0 = var.yres / 4 * 3, w = var.xres / 5, h = var.yres / 5;
    if (fill_only) {
        uint32_t px = ((uint32_t *)(fb + (size_t)(y0 + h / 2) * fix.line_length))[x0 + w / 2];
        printf("fill %08x\n", px);
        hold();
        int un = munmap(fb, len) == 0 ? 0 : -errno;
        printf("munmap %d\n", un);
        close(fd);
        return 0;
    }
    int drawn[2];
    if (pipe(drawn) != 0) return fail("pipe");
    pid_t pid = fork();
    if (pid < 0) return fail("fork");
    if (pid == 0) {
        fill(fb, x0, y0, w, h, rgb(0xe8, 0x70, 0x2c));
        if (write(drawn[1], "d", 1) != 1) _exit(1);
        hold();
        _exit(0);
    }
    close(drawn[1]);
    char c;
    (void)read(drawn[0], &c, 1); /* the child has drawn, or is gone */
    uint32_t px = ((uint32_t *)(fb + (size_t)(y0 + h / 2) * fix.line_length))[x0 + w / 2];
    printf("private %d child %08x\n", private, px);
    hold();
    int st;
    if (waitpid(pid, &st, 0) != pid) return fail("waitpid");
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        errno = ECHILD;
        return fail("the drawing child did not exit 0");
    }
    int un = munmap(fb, len) == 0 ? 0 : -errno;
    printf("munmap %d\n", un);
    close(fd);
    return 0;
}
