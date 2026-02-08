# WebSocket API (Draft)

This repo exposes a websocket gateway intended for external contributors to build clients, bots, and tools.

Server: `apps/ws_gateway`

Two streaming modes are supported:

1. JSON object stream (text frames)
2. FlatBuffers frame stream (binary frames)

## JSON Mode (Text Frames)

Connect:

- default: `ws://HOST:PORT/v1/json`

Client -> server messages (each websocket text frame is one JSON object):

- `{"op":"attach","name":"Alice","is_bot":false}`
- `{"op":"input","line":"look"}`
- `{"op":"detach"}`
- `{"op":"ping"}`

Server -> client messages:

- `{"op":"hello","mode":"json"}`
- `{"op":"attached","session":"<hex-32>"}`
- `{"op":"output","text":"...\\r\\n"}`
- `{"op":"err","text":"...\\r\\n"}`
- `{"op":"pong"}`

Notes:

- Output is currently line-oriented text, mirroring what a telnet user sees.
- The API is intentionally thin: you send game commands as strings and receive output/events as strings.

## FlatBuffers Mode (Binary Frames)

Connect:

- recommended: `ws://HOST:PORT/v1/fbs`

Each websocket binary frame is a FlatBuffers table:

```
table Frame {
  t: ubyte;
  session: [ubyte]; // length 16
  body: [ubyte];
}
root_type Frame;
```

Semantics:

- `t` and `body` match the shard request/response types in `crates/mudproto/src/shard.rs`.
- `session` is a 16-byte session ID (u128 big-endian).

Requests:

- `t=REQ_ATTACH (0x01)`: `body = flags(1 byte) + name(utf-8 bytes)`
  - flags bit0 = is_bot
- `t=REQ_INPUT (0x03)`: `body = line bytes (utf-8, without trailing newline required)`
- `t=REQ_DETACH (0x02)`: `body = empty`

Responses:

- `t=RESP_OUTPUT (0x81)`: `body = output bytes (typically includes \\r\\n)`
- `t=RESP_ERR (0x82)`: `body = error bytes (typically includes \\r\\n)`

Bindings:

- FlatBuffers bindings for `Frame` live in `apps/ws_gateway/src/ws_fb.rs`.

## Reference Bot

`apps/bot_party` is a reference implementation of an external party helper bot that connects via the websocket gateway (JSON mode).

