/*
 * aurix_tcf_demo - test program for the AURIX TCF agent (VS Code debugging).
 *
 * A small call chain main -> compute -> accumulate -> scale -> square so the
 * call stack shows several frames with function names and source lines,
 * and the disassembly view has real code to decode.  The program runs from
 * the CPU0 PSPR (code) / DSPR (data) and never returns (there is no host to
 * return to).  It is loaded into RAM by tools/ram_load.js - the flashed
 * application stays untouched and is restored by tools/ram_restore.js.
 */

volatile unsigned int g_ticks;      /* loop counter (watch this one)   */
volatile int g_result;              /* last accumulate() result        */

__attribute__((noinline)) static int square(int x)
{
    return x * x;
}

__attribute__((noinline)) static int scale(int v, int k)
{
    int s = square(v);
    return s * k + 1;
}

__attribute__((noinline)) static int accumulate(int n)
{
    int i;
    int sum = 0;

    for (i = 0; i < n; i++)
        sum += scale(i, 3);
    return sum;
}

__attribute__((noinline)) static void compute(void)
{
    g_result = accumulate(4);
    g_ticks++;
}

#ifdef ENABLE_CACHES
/* The TC3xx: CPU0's watchdog, the CSFRs PCON0 and DCON0 (bit 1, the cache bypassed). Out of
 * reset both caches are bypassed, and only CPU0 can clear its own ENDINIT. */
#define REG(a) (*(volatile unsigned int *)(a))
#define WDTCPU0CON0 0xF003624Cu

/* Sets ENDINIT (TC3xx user's manual, the watchdog password access, then a modify access). */
static void cpu0_endinit(unsigned int endinit)
{
    unsigned int value = REG(WDTCPU0CON0);
    unsigned int password = (value ^ 0xFCu) & 0xFFFCu;
    unsigned int reload = value & 0xFFFF0000u;

    if (value & 0x2u)
        REG(WDTCPU0CON0) = reload | password | 0x1u;
    REG(WDTCPU0CON0) = reload | password | 0x2u | endinit;
    while ((REG(WDTCPU0CON0) & 0x1u) != endinit)
        ;
}

/* The program cache and the data cache on, as the startup of an application does. */
static void caches_on(void)
{
    cpu0_endinit(0);
    __asm__ volatile("mtcr 0x920C, %0\n\tisync" : : "d"(0));
    __asm__ volatile("mtcr 0x9040, %0\n\tisync" : : "d"(0));
    cpu0_endinit(1);
}
#endif

int main(void)
{
#ifdef ENABLE_CACHES
    caches_on();
#endif
    for (;;)
        compute();
}
