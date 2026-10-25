/*
 * orti_demo - a tiny cooperative "OS" for the ORTI view of the debugger (TC275 CPU0, RAM only).
 *
 * Code in the CPU0 PSPR, data in the CPU0 DSPR: start_in_flash writes it to RAM, the flash is
 * not touched. Three tasks run in turn, each for about 0.4 s (STM0 at 100 MHz); a "timer ISR"
 * (a function called every 10 ms, no interrupt) counts ticks. orti_demo.ort describes the
 * variables below as ORTI 2.2 objects:
 *
 *   OsCurrentTask    the running task (0..2, 0xFF none)          OS RUNNINGTASK
 *   OsCurrentIsr     the running ISR (1 in the timer ISR, else 0)  OS RUNNINGISR2
 *   OsTaskState[3]   RUNNING 0, READY 1, WAITING 2, SUSPENDED 3   TASK STATE
 *   OsTcb[3]         state, prio, activations per task             TASK PRIORITY, vs_ACTIVATIONS
 *   OsRunningTcb     pointer to the running task's OsTcb           OS vs_RUNNING_STATE (->)
 *   OsTicks          ticks of the timer ISR                        OS vs_TICKS
 *
 * os_dispatch() is where the running task changes: a breakpoint there stops at each switch.
 */

#define TASKS 3
#define NO_TASK 0xFFu
#define RUNNING 0u
#define READY 1u
#define WAITING 2u
#define SUSPENDED 3u

/* STM0 timer, low 32 bits (TC27x: STM0 base 0xF0000000, TIM0 at +0x10), 100 MHz. */
#define STM0_TIM0 (*(volatile unsigned *)0xF0000010u)
#define SLICE 40000000u /* 0.4 s */
#define TICK 1000000u   /* 10 ms */

typedef struct {
    volatile unsigned char state;
    unsigned char prio;
    volatile unsigned activations;
} OsTcbType;

volatile unsigned char OsCurrentTask;
volatile unsigned char OsCurrentIsr;
volatile unsigned char OsTaskState[TASKS];
OsTcbType OsTcb[TASKS];
OsTcbType *volatile OsRunningTcb;
volatile unsigned OsTicks;
volatile unsigned g_work[TASKS];

static unsigned last_tick;

static void set_state(unsigned task, unsigned char state)
{
    OsTaskState[task] = state;
    OsTcb[task].state = state;
}

__attribute__((noinline)) static void timer_isr(void)
{
    OsCurrentIsr = 1;
    OsTicks++;
    OsCurrentIsr = 0;
}

/* Runs task `task` for one slice: its work, and the timer ISR every tick. */
static void run_slice(unsigned task)
{
    unsigned start = STM0_TIM0;
    while (STM0_TIM0 - start < SLICE) {
        g_work[task]++;
        if (STM0_TIM0 - last_tick >= TICK) {
            last_tick = STM0_TIM0;
            timer_isr();
        }
    }
}

__attribute__((noinline)) static void TaskA(void) { run_slice(0); }
__attribute__((noinline)) static void TaskB(void) { run_slice(1); }
__attribute__((noinline)) static void TaskC(void) { run_slice(2); }

/* The running task changes to `task`; the one before is ready again (TaskC waits instead). */
__attribute__((noinline)) static void os_dispatch(unsigned task, unsigned previous)
{
    if (previous < TASKS)
        set_state(previous, previous == 2 ? WAITING : READY);
    OsCurrentTask = (unsigned char)task;
    OsRunningTcb = &OsTcb[task];
    OsTcb[task].activations++;
    set_state(task, RUNNING);
}

int main(void)
{
    unsigned task;
    unsigned previous = NO_TASK;

    /* No start code clears the data: set it here. */
    for (task = 0; task < TASKS; task++) {
        OsTcb[task].prio = (unsigned char)(3 - task);
        OsTcb[task].activations = 0;
        g_work[task] = 0;
        set_state(task, SUSPENDED);
    }
    OsCurrentTask = NO_TASK;
    OsCurrentIsr = 0;
    OsRunningTcb = 0;
    OsTicks = 0;
    last_tick = STM0_TIM0;

    for (;;) {
        for (task = 0; task < TASKS; task++) {
            os_dispatch(task, previous);
            previous = task;
            if (task == 0)
                TaskA();
            else if (task == 1)
                TaskB();
            else
                TaskC();
        }
    }
}
