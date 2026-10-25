//! Shows the caches of every core of a probe-rs session, as app B's Cache tab does, while the
//! target runs, then measures once: the hits and misses, the instructions per cycle, the stalls.
//!
//! ```text
//! cargo run --example cache_view -- <attach.json> <chip>
//! ```
//!
//! `chip` picks the cores (`TC275`, `TC36x`). The measurement writes `CCTRL` of each core and
//! puts it back; nothing else is written. The target is left halted.

mod common;

use std::time::Duration;

use aurix_debug_model::cache::{CacheView, Caches};
use aurix_debug_model::live_watch::INTERVAL_MS;
use aurix_debug_model::protocol::{ReadRanges, ReadRangesArguments};
use dap_types::requests::WriteMemory;

fn read(
    client: &aurix_dap_client::DapClient,
    view: &mut CacheView,
    core: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    for write in view.take_writes() {
        client.request::<WriteMemory>(write.arguments(), common::TIMEOUT)?;
    }
    let (ranges, generation) = view.ranges();
    match client.request::<ReadRanges>(ReadRangesArguments { ranges, core }, common::TIMEOUT) {
        Ok(response) => view.answered(generation, &response.ranges),
        Err(error) => view.failed(&error.to_string()),
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: cache_view <attach.json> <chip>";
    let attach = args.next().ok_or(usage)?;
    let chip = args.next().ok_or(usage)?;
    let caches = Caches::for_chip(&chip).ok_or("no cache view for this chip")?;
    let mut view = CacheView::new(caches);

    let client = common::start(&attach)?;
    let core = common::threads(&client)?.first().map(|(id, _)| *id);
    read(&client, &mut view, core)?;
    if let Some(error) = view.error() {
        println!("error: {error}");
    }
    for row in view.rows() {
        match row {
            Ok(row) => println!(
                "CPU{} {}: {}; {}; code cached {}, data cached {}",
                row.core,
                row.kind.name(),
                row.program,
                row.data,
                row.code_segments,
                row.data_segments
            ),
            Err(error) => println!("error: {error}"),
        }
    }

    if view.measure() {
        while view.measuring() {
            std::thread::sleep(Duration::from_millis(INTERVAL_MS));
            read(&client, &mut view, core)?;
        }
        // The writes that put CCTRL back.
        read(&client, &mut view, core)?;
        for row in view.rows().into_iter().flatten() {
            if let Some(m) = row.measured {
                println!("CPU{}: {}; {}", row.core, m.program, m.data);
                println!("CPU{}: {}", row.core, m.bench);
            }
        }
    }
    if let Some(error) = view.error() {
        println!("error: {error}");
    }
    common::stop(&client);
    Ok(())
}
