Using `cargo run` you can see the text messages

Using probe-rs vscode debug config, you are able to set breakpoints outside of main loop

launch.json -> configurations -> probe-rs-debug -> flashingConfig -> haltAfterReset should make halting in main possible, does not work right now