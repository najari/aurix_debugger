/*
 * reset_demo start code: the reset enters here (the jump at 0x80000020 in PF0, after the boot
 * mode header, points to this address in the PF1 test sector).
 *
 * Sets PSW, the stack and PCXI = 0 (outermost frame), links the CSA free list (64 contexts in
 * the CPU0 DSPR) and jumps to _c_start, which disables the watchdogs and calls core0_main.
 */
    .section .text.start, "ax"
    .globl _start
    .type _start, @function
_start:
    /* The boot software enters through the non-cached alias (0xA...): continue at the link
     * address (0x8...), where the debug information and the breakpoints are. */
    movh.a  %a15, hi:1f
    lea     %a15, [%a15] lo:1f
    ji      %a15
1:
    /* PSW: supervisor mode, global registers on, call depth counting off (as the
     * application's start code). */
    mov     %d0, 0x980
    dsync
    mtcr    $psw, %d0
    isync

    movh.a  %sp, hi:__stack_top
    lea     %sp, [%sp] lo:__stack_top

    /* PCXI = 0 and A11 = 0: _c_start is the outermost frame of the call stack. */
    mov     %d0, 0
    mov.a   %a11, %d0
    dsync
    mtcr    $pcxi, %d0
    isync

    /* CSA free list: each context holds the link word of the next one,
     * link(addr) = ((addr >> 12) & 0xF0000) | ((addr >> 6) & 0xFFFF). */
    movh.a  %a2, hi:__csa_start
    lea     %a2, [%a2] lo:__csa_start
    mov     %d4, __csa_count - 1
    movh    %d3, 0xF
1:  lea     %a3, [%a2] 64
    mov.d   %d0, %a3
    sh      %d1, %d0, -12
    and     %d1, %d3
    extr.u  %d2, %d0, 6, 16
    or      %d1, %d2
    st.w    [%a2], %d1
    mov.aa  %a2, %a3
    add     %d4, -1
    jnz     %d4, 1b
    mov     %d1, 0
    st.w    [%a2], %d1

    /* FCX = the first context, LCX = the fourth last one (a reserve for a trap on
     * CSA depletion). */
    movh.a  %a2, hi:__csa_start
    lea     %a2, [%a2] lo:__csa_start
    mov.d   %d0, %a2
    sh      %d1, %d0, -12
    and     %d1, %d3
    extr.u  %d2, %d0, 6, 16
    or      %d1, %d2
    dsync
    mtcr    $fcx, %d1
    isync

    movh.a  %a2, hi:__csa_lcx
    lea     %a2, [%a2] lo:__csa_lcx
    mov.d   %d0, %a2
    sh      %d1, %d0, -12
    and     %d1, %d3
    extr.u  %d2, %d0, 6, 16
    or      %d1, %d2
    dsync
    mtcr    $lcx, %d1
    isync

    j       _c_start
    .size _start, . - _start
