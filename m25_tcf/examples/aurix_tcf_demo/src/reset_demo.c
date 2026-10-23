/*
 * reset_demo - the demo call chain as an image that the reset starts, with core0_main as the
 * entry function (as in iLLD based applications).
 *
 * reset_start.S sets up the stack and the CSA list and jumps to _c_start, which disables the
 * Safety and CPU0 watchdogs, clears .bss and calls core0_main. Only CPU0 runs: CPU1 and CPU2
 * stay halted after the reset. The clocks stay at their reset values.
 */

#define REG(address) (*(volatile unsigned int *)(address))

/* WDTxCON0; WDTxCON1 follows at + 4. */
#define WDTSCON0 0xF00360F0u
#define WDTCPU0CON0 0xF0036100u
#define WDT_CON1_DR 0x8u

volatile unsigned int g_ticks;      /* loop counter (watch this one)   */
volatile int g_result;              /* last accumulate() result        */

/* Sets ENDINIT of a watchdog (TC27x user's manual 7.4.4.3): a password access (LCK = 0,
 * ENDINIT = 1, PW with bits 7:2 back to their true value, REL as it is), then a modify access
 * (LCK = 1, the new ENDINIT). */
static void wdt_set_endinit(unsigned int con0, unsigned int endinit)
{
    unsigned int value = REG(con0);
    unsigned int password = (value ^ 0xFCu) & 0xFFFCu;
    unsigned int reload = value & 0xFFFF0000u;

    if (value & 0x2u)
        REG(con0) = reload | password | 0x1u;
    REG(con0) = reload | password | 0x2u | endinit;
    while ((REG(con0) & 0x1u) != endinit)
        ;
}

static void wdt_disable(unsigned int con0)
{
    wdt_set_endinit(con0, 0);
    REG(con0 + 4) |= WDT_CON1_DR;
    wdt_set_endinit(con0, 1);
}

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

int core0_main(void)
{
    for (;;)
        compute();
}

extern unsigned int __bss_start[], __bss_end[];

void _c_start(void) __attribute__((noreturn));
void _c_start(void)
{
    unsigned int *p;

    wdt_disable(WDTSCON0);
    wdt_disable(WDTCPU0CON0);
    for (p = __bss_start; p < __bss_end; p++)
        *p = 0;
    core0_main();
    for (;;)
        ;
}
