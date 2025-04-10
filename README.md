# Different examples how to use no_std Rust on the ESP32C6

## Devcontainers

As im using a immutable linux distro i had to use devcontainers for development. The necessary file can be found under `current_configs/devcontainers` as [devcontainer.json](current_configs/devcontainers/devcontainer.json) and [Dockerfile](current_configs/devcontainers/Dockerfile). You probably want to setup the [probe-rs](https://probe.rs/) udev [rules](https://probe.rs/docs/getting-started/probe-setup/#linux%3A-udev-rules) to allow non root users access to the esp32c6 debug probe.

## Launch configuration

A VSCode [launch.json](current_configs/vscode/launch.json) is provided to start the debugger using the [VSCode Extension](https://marketplace.visualstudio.com/items?itemName=probe-rs.probe-rs-debugger). More information can be found in the [probe-rs documentation](https://probe.rs/docs/tools/debugger/).

[task.json](current_configs/vscode/tasks.json) is needed as well to be able to compile the project before flashing to the esp.

To be able to launch the project with `cargo run`, a [config.toml](current_configs/cargo/config.toml) is provided to use probe-rs.




## Tools and resources used

- [esp-generate](https://github.com/esp-rs/esp-generate): to generate skeleton projects (the provided devcontainer configs didnt work for me)
- [probe-rs](https://probe.rs/): for compiling and debugging
- different examples from the used rust packages like [smart-leds](https://github.com/smart-leds-rs/smart-leds), [esp-hal-smartled](https://github.com/esp-rs/esp-hal-community/tree/main/esp-hal-smartled), etc.


