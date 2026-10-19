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

int main(void)
{
    for (;;)
        compute();
}
