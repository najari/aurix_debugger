/*
 * gtm_demo - two GTM MCS channels running small programs, for the GTM view of the debugger
 * (TC275 CPU0, RAM only).
 *
 * Code in the CPU0 PSPR, data in the CPU0 DSPR: start_in_flash writes it to RAM, the flash is
 * not touched. main() writes the MCS program below into the memory of MCS1 (MCS address 0 is
 * 0xF0140000; MCS1 keeps the default 4 KB + 2 KB layout on this board, where MCS3 borrows
 * MCS0's page), sets the program counters and the stack pointer, and enables channels 0 and 1.
 * The GTM must be on (GTM_CLC.DISS = 0): the boot application switches it on; without it
 * g_gtm_state says so and nothing is started. The device is reset after the test.
 *
 *   channel 0, from 0x000:  R0 counts up and is stored at MCS address 0x400
 *   channel 1, from 0x020:  R2 counts up by 3 and is stored at 0x404; a subroutine at 0x040
 *                           counts R3 (CALL/RET, stack in R7 from 0x600, SP_CNT 1 inside)
 *
 * The TriCore copies the two stored words to g_mcs_count and g_mcs_sum and counts g_ticks.
 */

#define GTM_BASE 0xF0100000u
#define GTM_CLC (*(volatile unsigned *)(GTM_BASE + 0x9FD00u))
#define MCS1_REGS (GTM_BASE + 0x31000u)
#define MCS1_MEM ((volatile unsigned *)(GTM_BASE + 0x40000u))
#define CH_R(ch, r) (*(volatile unsigned *)(MCS1_REGS + (ch) * 0x80u + (r) * 4u))
#define CH_CTRL(ch) (*(volatile unsigned *)(MCS1_REGS + (ch) * 0x80u + 0x20u))
#define CH_PC(ch) (*(volatile unsigned *)(MCS1_REGS + (ch) * 0x80u + 0x40u))

/* The MCS program, word by word (TC27x User's Manual 25.13.2). */
static const struct {
    unsigned address;
    unsigned code;
} program[] = {
    /* channel 0 */
    {0x000, 0x10000000u}, /* MOVL R0, 0 */
    {0x004, 0x20000001u}, /* ADDL R0, 1 */
    {0x008, 0xA0020400u}, /* MWR R0, 0x400 */
    {0x00C, 0xE0000004u}, /* JMP 0x004 */
    /* channel 1 */
    {0x020, 0x12000010u}, /* MOVL R2, 0x10 */
    {0x024, 0x22000003u}, /* ADDL R2, 3 */
    {0x028, 0xA2020404u}, /* MWR R2, 0x404 */
    {0x02C, 0xE0030040u}, /* CALL 0x040 */
    {0x030, 0xE0000024u}, /* JMP 0x024 */
    /* subroutine of channel 1 */
    {0x040, 0x23000001u}, /* ADDL R3, 1 */
    {0x044, 0xE0040000u}, /* RET */
};

volatile unsigned g_gtm_state;  /* 0 running, 1 the GTM is off */
volatile unsigned g_mcs_count;  /* MCS address 0x400, written by channel 0 */
volatile unsigned g_mcs_sum;    /* MCS address 0x404, written by channel 1 */
volatile unsigned g_ticks;

static void start_mcs(void)
{
    unsigned i;

    /* Both channels off while their memory and registers are written. */
    CH_CTRL(0) = 0;
    CH_CTRL(1) = 0;
    for (i = 0; i < sizeof program / sizeof program[0]; i++)
        MCS1_MEM[program[i].address / 4] = program[i].code;
    MCS1_MEM[0x400 / 4] = 0;
    MCS1_MEM[0x404 / 4] = 0;

    CH_PC(0) = 0x000;
    CH_PC(1) = 0x020;
    for (i = 0; i < 8; i++) {
        CH_R(0, i) = 0;
        CH_R(1, i) = 0;
    }
    CH_R(1, 7) = 0x600; /* the stack of channel 1 */

    CH_CTRL(0) = 1;
    CH_CTRL(1) = 1;
}

int main(void)
{
    g_ticks = 0;
    g_mcs_count = 0;
    g_mcs_sum = 0;
    if (GTM_CLC & 2u) {
        g_gtm_state = 1;
        for (;;)
            g_ticks++;
    }
    g_gtm_state = 0;
    start_mcs();
    for (;;) {
        g_mcs_count = MCS1_MEM[0x400 / 4];
        g_mcs_sum = MCS1_MEM[0x404 / 4];
        g_ticks++;
    }
}
