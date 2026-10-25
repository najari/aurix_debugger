/*
 * rtt_demo - RTT output test for the probe-rs debugger (TC275, RAM only).
 *
 * Code in the PSPR, data in the DSPR of the core it is built for (CPU0 by default, CPU1
 * with build_rtt.ps1 -Core 1): start_in_flash writes it to RAM, the flash is not touched.
 * The program has a SEGGER RTT control block (_SEGGER_RTT, the debugger finds it by its
 * symbol) with one up channel "Terminal", and writes a line to it about twice a second:
 *
 *     rtt_demo CPU0 line 12, ticks 3456789
 *
 * and one down channel "Input": a line sent to it (up to the line break) comes back on the
 * up channel as
 *
 *     rtt_demo CPU0 got: hello
 *
 * The RTT code is a minimal one written for this test (the layout of the SEGGER
 * control block, no SEGGER source).  A full buffer drops the line (no blocking).
 */

#define UP_SIZE 512u
#define DOWN_SIZE 64u
/* The longest input line kept; the rest of a longer one is dropped. */
#define INPUT_SIZE 40u

/* The core the build runs on (build_rtt.ps1 -Core 1 builds for CPU1). */
#ifndef CORE_NAME
#define CORE_NAME "CPU0"
#endif

typedef struct {
    const char *name;
    char *buffer;
    unsigned size;
    volatile unsigned wr_off;
    volatile unsigned rd_off;
    unsigned flags;
} RttBuffer;

typedef struct {
    char id[16];
    int max_up;
    int max_down;
    RttBuffer up[1];
    RttBuffer down[1];
} RttControlBlock;

RttControlBlock _SEGGER_RTT;
static char up_buffer[UP_SIZE];
static volatile char down_buffer[DOWN_SIZE];

volatile unsigned g_ticks;    /* loop counter (watch this one)   */
volatile unsigned g_lines;    /* lines written to RTT             */
volatile unsigned g_dropped;  /* lines dropped (buffer full)      */
volatile unsigned g_received; /* bytes read from the down channel */

static char input[INPUT_SIZE];
static unsigned input_len;

/* STM0 timer, low 32 bits (TC27x: STM0 base 0xF0000000, TIM0 at +0x10). */
#define STM0_TIM0 (*(volatile unsigned *)0xF0000010u)

static void rtt_init(void)
{
    static const char id[] = "SEGGER RTT";
    unsigned i;

    _SEGGER_RTT.max_up = 1;
    _SEGGER_RTT.max_down = 1;
    _SEGGER_RTT.up[0].name = "Terminal";
    _SEGGER_RTT.up[0].buffer = up_buffer;
    _SEGGER_RTT.up[0].size = UP_SIZE;
    _SEGGER_RTT.up[0].wr_off = 0;
    _SEGGER_RTT.up[0].rd_off = 0;
    _SEGGER_RTT.up[0].flags = 0;
    _SEGGER_RTT.down[0].name = "Input";
    _SEGGER_RTT.down[0].buffer = (char *)down_buffer;
    _SEGGER_RTT.down[0].size = DOWN_SIZE;
    _SEGGER_RTT.down[0].wr_off = 0;
    _SEGGER_RTT.down[0].rd_off = 0;
    _SEGGER_RTT.down[0].flags = 0;
    /* The id last: a reader must not find a half written block. */
    for (i = 0; i < sizeof(_SEGGER_RTT.id); i++)
        _SEGGER_RTT.id[i] = i < sizeof(id) ? id[i] : 0;
}

/* Writes the n bytes of s to the up channel, or nothing when they do not fit. */
static int rtt_write(const char *s, unsigned n)
{
    RttBuffer *b = &_SEGGER_RTT.up[0];
    unsigned wr = b->wr_off;
    unsigned rd = b->rd_off;
    unsigned free = rd > wr ? rd - wr - 1 : b->size - (wr - rd) - 1;
    unsigned i;

    if (n > free)
        return 0;
    for (i = 0; i < n; i++) {
        b->buffer[wr] = s[i];
        wr = wr + 1 == b->size ? 0 : wr + 1;
    }
    b->wr_off = wr;
    return 1;
}

/* Appends the decimal digits of v to p, returns the end. */
static char *put_uint(char *p, unsigned v)
{
    char digits[10];
    int n = 0;

    do {
        digits[n++] = (char)('0' + v % 10);
        v /= 10;
    } while (v);
    while (n)
        *p++ = digits[--n];
    return p;
}

static char *put_text(char *p, const char *s)
{
    while (*s)
        *p++ = *s++;
    return p;
}

static void write_line(void)
{
    char line[48];
    char *p = line;

    p = put_text(p, "rtt_demo " CORE_NAME " line ");
    p = put_uint(p, g_lines + 1);
    p = put_text(p, ", ticks ");
    p = put_uint(p, g_ticks);
    *p++ = '\n';
    if (rtt_write(line, (unsigned)(p - line)))
        g_lines++;
    else
        g_dropped++;
}

/* The next byte the debugger wrote to the down channel, -1 if there is none. */
static int rtt_read(void)
{
    RttBuffer *b = &_SEGGER_RTT.down[0];
    unsigned rd = b->rd_off;
    int c;

    if (rd == b->wr_off)
        return -1;
    c = (unsigned char)down_buffer[rd];
    b->rd_off = rd + 1 == b->size ? 0 : rd + 1;
    return c;
}

/* Sends the input line back on the up channel. */
static void echo_input(void)
{
    char line[64];
    char *p = line;
    unsigned i;

    p = put_text(p, "rtt_demo " CORE_NAME " got: ");
    for (i = 0; i < input_len; i++)
        *p++ = input[i];
    *p++ = '\n';
    if (!rtt_write(line, (unsigned)(p - line)))
        g_dropped++;
}

/* Reads what the debugger sent, a line at a time. */
static void poll_input(void)
{
    int c;

    while ((c = rtt_read()) >= 0) {
        g_received++;
        if (c == '\n' || c == '\r') {
            if (input_len)
                echo_input();
            input_len = 0;
        } else if (input_len < INPUT_SIZE) {
            input[input_len++] = (char)c;
        }
    }
}

int main(void)
{
    unsigned last;

    rtt_init();
    last = STM0_TIM0;
    for (;;) {
        g_ticks++;
        poll_input();
        /* About twice a second at the STM clock of 100 MHz. */
        if (STM0_TIM0 - last >= 50000000u) {
            last = STM0_TIM0;
            write_line();
        }
    }
}
