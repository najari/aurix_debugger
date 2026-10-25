/*
 * aurix_trace.h - a software event trace for the AURIX debugger (SW Trace tab, aurix-run
 * `trace expect`): the program writes 8-byte records to an RTT up channel, the debugger reads
 * them while the core runs (probe-rs RTT, channel format BinaryLE) and shows when each event
 * happened. What TRACE32 calls a LOGGER trace; it needs no Emulation Device.
 *
 * A record, little endian:
 *
 *     offset 0  u32  STM0 TIM0 (100 MHz on the TC27x: 10 ns, once round in 42.9 s)
 *     offset 4  u16  id: bit 15 set for the end of a span (AURIX_TRACE_END)
 *     offset 6  u16  arg
 *
 * A record that does not fit is dropped and counted; when there is room again, one record
 * AURIX_TRACE_ID_DROPPED with the count (at most 0xFFFF) comes first. A record is written with
 * the interrupts off, so an ISR can trace too; each core has its own channel.
 *
 * Use, in one C file:
 *
 *     #define AURIX_TRACE_IMPLEMENTATION
 *     #define AURIX_TRACE_OWN_RTT      // no RTT block of its own: this one, channel "SwTrace"
 *     #include "aurix_trace.h"
 *
 *     aurix_trace_init();              // before the first record
 *     AURIX_TRACE_BEGIN(1); work(); AURIX_TRACE_END(1);
 *     AURIX_TRACE(20, value);          // a single event with a value
 *
 * A program with its own RTT control block leaves out AURIX_TRACE_OWN_RTT and hands one of its
 * up buffers to aurix_trace_attach() instead. The debugger names the ids from a file next to the
 * attach file (`<attach>.trace`, `id = name` a line).
 */
#ifndef AURIX_TRACE_H
#define AURIX_TRACE_H

/* The end bit of an id, and the id of the record that says how many were dropped. */
#define AURIX_TRACE_END_BIT 0x8000u
#define AURIX_TRACE_ID_DROPPED 0x7FFFu
#define AURIX_TRACE_RECORD_SIZE 8u

#ifndef AURIX_TRACE_BUFFER_SIZE
#define AURIX_TRACE_BUFFER_SIZE 1024u
#endif

/* STM0 TIM0: 0xF0000010 on the TC27x, 0xF0001010 on the TC3xx. */
#ifndef AURIX_TRACE_STM_ADDRESS
#define AURIX_TRACE_STM_ADDRESS 0xF0000010u
#endif

/* An RTT buffer as the SEGGER control block lays it out. */
typedef struct {
    const char *name;
    unsigned char *buffer;
    unsigned size;
    volatile unsigned wr_off;
    volatile unsigned rd_off;
    unsigned flags;
} AurixRttBuffer;

/* Records dropped since the last AURIX_TRACE_ID_DROPPED record. */
extern volatile unsigned aurix_trace_dropped;

void aurix_trace_attach(AurixRttBuffer *up);
void aurix_trace_init(void);
void aurix_trace(unsigned id, unsigned arg);

#define AURIX_TRACE(id, arg) aurix_trace((id), (arg))
#define AURIX_TRACE_BEGIN(id) aurix_trace((id), 0u)
#define AURIX_TRACE_END(id) aurix_trace((unsigned)(id) | AURIX_TRACE_END_BIT, 0u)

#ifdef AURIX_TRACE_IMPLEMENTATION

volatile unsigned aurix_trace_dropped;
static AurixRttBuffer *aurix_trace_up;

#ifdef AURIX_TRACE_OWN_RTT
typedef struct {
    char id[16];
    int max_up;
    int max_down;
    AurixRttBuffer up[1];
    AurixRttBuffer down[1];
} AurixRttControlBlock;

AurixRttControlBlock _SEGGER_RTT;
static unsigned char aurix_trace_buffer[AURIX_TRACE_BUFFER_SIZE];
#endif

void aurix_trace_attach(AurixRttBuffer *up)
{
    aurix_trace_up = up;
}

void aurix_trace_init(void)
{
#ifdef AURIX_TRACE_OWN_RTT
    static const char id[] = "SEGGER RTT";
    unsigned i;

    _SEGGER_RTT.max_up = 1;
    _SEGGER_RTT.max_down = 0;
    _SEGGER_RTT.up[0].name = "SwTrace";
    _SEGGER_RTT.up[0].buffer = aurix_trace_buffer;
    _SEGGER_RTT.up[0].size = AURIX_TRACE_BUFFER_SIZE;
    _SEGGER_RTT.up[0].wr_off = 0;
    _SEGGER_RTT.up[0].rd_off = 0;
    _SEGGER_RTT.up[0].flags = 0; /* no blocking: a full buffer drops */
    /* The id last: a reader must not find a half written block. */
    for (i = 0; i < sizeof(_SEGGER_RTT.id); i++)
        _SEGGER_RTT.id[i] = i < sizeof(id) ? id[i] : 0;
    aurix_trace_up = &_SEGGER_RTT.up[0];
#endif
    aurix_trace_dropped = 0;
}

/* Room in bytes for the writer of b. */
static unsigned aurix_trace_room(const AurixRttBuffer *b, unsigned wr)
{
    unsigned rd = b->rd_off;
    return rd > wr ? rd - wr - 1u : b->size - (wr - rd) - 1u;
}

/* Puts one record at wr, returns the offset after it. */
static unsigned aurix_trace_put(AurixRttBuffer *b, unsigned wr, unsigned time, unsigned id,
                                unsigned arg)
{
    unsigned char record[AURIX_TRACE_RECORD_SIZE];
    unsigned i;

    record[0] = (unsigned char)time;
    record[1] = (unsigned char)(time >> 8);
    record[2] = (unsigned char)(time >> 16);
    record[3] = (unsigned char)(time >> 24);
    record[4] = (unsigned char)id;
    record[5] = (unsigned char)(id >> 8);
    record[6] = (unsigned char)arg;
    record[7] = (unsigned char)(arg >> 8);
    for (i = 0; i < AURIX_TRACE_RECORD_SIZE; i++) {
        b->buffer[wr] = record[i];
        wr = wr + 1u == b->size ? 0u : wr + 1u;
    }
    return wr;
}

void aurix_trace(unsigned id, unsigned arg)
{
    AurixRttBuffer *b = aurix_trace_up;
    unsigned ie, wr, time, need;

    if (!b)
        return;
    /* DISABLE saves ICR.IE, RESTORE puts it back (TC1.6). */
    __asm__ volatile("disable %0" : "=d"(ie)::"memory");
    time = *(volatile unsigned *)AURIX_TRACE_STM_ADDRESS;
    wr = b->wr_off;
    need = aurix_trace_dropped ? 2u * AURIX_TRACE_RECORD_SIZE : AURIX_TRACE_RECORD_SIZE;
    if (aurix_trace_room(b, wr) < need) {
        aurix_trace_dropped++;
    } else {
        if (aurix_trace_dropped) {
            unsigned n = aurix_trace_dropped > 0xFFFFu ? 0xFFFFu : aurix_trace_dropped;
            wr = aurix_trace_put(b, wr, time, AURIX_TRACE_ID_DROPPED, n);
            aurix_trace_dropped = 0;
        }
        wr = aurix_trace_put(b, wr, time, id, arg);
        /* The records first, the offset last: the reader takes only whole records. */
        __asm__ volatile("dsync" ::: "memory");
        b->wr_off = wr;
    }
    __asm__ volatile("restore %0" ::"d"(ie) : "memory");
}

#endif /* AURIX_TRACE_IMPLEMENTATION */
#endif /* AURIX_TRACE_H */
