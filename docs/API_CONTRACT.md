# OmniProxy WebSocket API Contract

Version: `v0`  
Transport: WebSocket (text JSON messages)

Default endpoint:

```txt
ws://127.0.0.1:9091
```

## Event Envelope

Every event is a JSON object with a `type` discriminator.

```json
{
  "type": "HttpRequest | HttpResponse | WebSocketFrame",
  "...": "event specific fields"
}
```

## HttpRequest

```json
{
  "type": "HttpRequest",
  "timestamp_ms": 1710000000000,
  "request_id": "optional-string",
  "client": "127.0.0.1:53742",
  "method": "GET",
  "uri": "https://example.com/api",
  "headers": [["host", "example.com"]],
  "body_b64": "optional-base64",
  "body_truncated": false,
  "body_size": 123,
  "body_capture_reason": "optional-string"
}
```

Field semantics:
1. `request_id`: correlation key injected by OmniProxy (`x-omni-request-id`).
2. `body_b64`: present when body capture policy allows it.
3. `body_truncated`: true when omitted due size/compression/policy.
4. `body_capture_reason` possible values:
   1. `sampled_out`
   2. `compressed_skipped`
   3. `unknown_length_streaming`
   4. `over_limit`

## HttpResponse

```json
{
  "type": "HttpResponse",
  "timestamp_ms": 1710000000001,
  "request_id": "optional-string",
  "client": "127.0.0.1:53742",
  "status": 200,
  "headers": [["content-type", "application/json"]],
  "body_b64": "optional-base64",
  "body_truncated": false,
  "body_size": 456,
  "body_capture_reason": "optional-string"
}
```

Field semantics:
1. `request_id` + `client` are used for correlation.
2. Body fields follow request semantics.

## WebSocketFrame

```json
{
  "type": "WebSocketFrame",
  "timestamp_ms": 1710000000002,
  "client": null,
  "kind": "text",
  "payload_len": 42,
  "preview": "hello world"
}
```

`kind` may be:
1. `text`
2. `binary`
3. `ping`
4. `pong`
5. `close`
6. `frame`

## Backpressure Behavior

OmniProxy uses broadcast fan-out.  
When a client lags and dropped-event total exceeds `--api-max-lag` (`OMNI_API_MAX_LAG`), the lagging WS API connection is closed.

## Compatibility Notes

1. Unknown fields should be ignored by consumers.
2. Additional event types may be introduced in future versions.
3. `timestamp_ms` is Unix epoch milliseconds.
