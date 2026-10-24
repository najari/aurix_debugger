# probe-rs debugger extension for Zed

> **Modified copy.** Taken from [probe-rs/zed-extension](https://github.com/probe-rs/zed-extension)
> at `1570bed` (MIT OR Apache-2.0, see `LICENSE-MIT` and `LICENSE-APACHE`). Changed on 2026-10-24:
> `get_dap_binary` sends the request kind of the configuration (`launch` or `attach`) instead of
> always `launch` (`src/lib.rs`). Before the change an `attach` scenario reset the target, because
> probe-rs resets on a launch.
>
> Build and install over the registry copy (Zed closed):
>
> ```powershell
> rustup target add wasm32-wasip2
> cargo build --release --target wasm32-wasip2
> Copy-Item target\wasm32-wasip2\release\zed_probe_rs.wasm "$env:LOCALAPPDATA\Zed\extensions\installed\probe-rs\extension.wasm"
> ```

Debugger extension for [Zed](https://zed.dev) which enables the use of [probe-rs](https://probe.rs) as a debugger.

Currently still in development.

## Debugger configuration

A minimal configuration in `.zed/debug.json`:

```json
[
  {
    "label": "Launch probe-rs debugging",
    "adapter": "probe-rs",
    "request": "launch",
    "cwd": "$ZED_WORKTREE_ROOT",
    "server": "127.0.0.1:50000  ",
    "coreConfigs": []
  }
]
```

This will attach to an already running local instance of probe-rs, listening on port 50000.

The configuration options are the same as in the vscode debugger extensions, see the [probe-rs docs](https://probe.rs/docs/tools/debugger/) for more information.


## Development setup

See the Zed documentation on extension development: <https://zed.dev/docs/extensions/developing-extensions>.
