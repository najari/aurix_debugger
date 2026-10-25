/*
 * trace_cpu1 - a software trace on CPU1 for the SW Trace tab (TC275 CPU1, RAM only), next to the
 * ORTI demo's trace on CPU0 (build_trace_cpu1.ps1, build\trace_cpu1.elf).
 *
 * Code in the CPU1 PSPR, data in the CPU1 DSPR: start_in_flash --core 1 writes it to RAM. It
 * writes far more records than the ORTI demo, to load the reading: every 1 ms a "job" of about
 * 100 us (id 30, begin and end) and every 5 ms a "sample" (id 31, the job count as arg), 2,200
 * records (17.6 KB) a second, into a 4 KiB RTT buffer. aurix_trace_high says how full it got.
 */

#define AURIX_TRACE_IMPLEMENTATION
#define AURIX_TRACE_OWN_RTT
#define AURIX_TRACE_BUFFER_SIZE 4096u
#include "aurix_trace.h"

#define STM0_TIM0 (*(volatile unsigned *)0xF0000010u)
#define MS 100000u /* STM0 at 100 MHz */
#define JOB_TICKS 10000u

#define TRACE_JOB 30u
#define TRACE_SAMPLE 31u

volatile unsigned g_jobs;

static void job(void)
{
    unsigned start = STM0_TIM0;
    AURIX_TRACE_BEGIN(TRACE_JOB);
    while (STM0_TIM0 - start < JOB_TICKS)
        ;
    g_jobs++;
    AURIX_TRACE_END(TRACE_JOB);
}

int main(void)
{
    unsigned next;

    aurix_trace_init();
    g_jobs = 0;
    next = STM0_TIM0 + MS;
    for (;;) {
        while ((int)(STM0_TIM0 - next) < 0)
            ;
        next += MS;
        job();
        if (g_jobs % 5 == 0)
            AURIX_TRACE(TRACE_SAMPLE, g_jobs & 0xFFFFu);
    }
}
