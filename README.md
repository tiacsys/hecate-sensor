# Hecate Sensor

This repository contains firmware for an ESP32 based sensor unit, which sends
data to a hecate server via websocket connection.

## Building and Flashing

To build this firmware, some esp-rs infrastructure, such as rustc targets, must
be installed. The easiest way to do this is by using `espup`:

```
cargo install espup
espup install
```

With the esp-rs infrastructure in place, you can connect a sensor unit and run
`cargo run` to build and flash the firmware, and open a serial connection to the
unit showing log output.
