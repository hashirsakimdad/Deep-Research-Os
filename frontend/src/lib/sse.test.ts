import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSse } from "./sse.ts";

test("parses complete messages with ids and keeps the incomplete tail", () => {
  const { messages, rest } = parseSse('id: 1\ndata: {"a":1}\n\nid: 2\ndata: {"b":2}\n\nid: 3\ndata: {"c"');
  assert.deepEqual(messages, [
    { id: "1", data: '{"a":1}' },
    { id: "2", data: '{"b":2}' },
  ]);
  assert.equal(rest, 'id: 3\ndata: {"c"');
});

test("a message split across network chunks is reassembled", () => {
  const first = parseSse('id: 7\ndata: {"x"');
  assert.equal(first.messages.length, 0);
  const second = parseSse(first.rest + ":1}\n\n");
  assert.deepEqual(second.messages, [{ id: "7", data: '{"x":1}' }]);
  assert.equal(second.rest, "");
});

test("ignores keep-alive comments and handles CRLF line endings", () => {
  const { messages } = parseSse(": keep-alive\n\nid: 4\r\ndata: hello\r\n\r\n");
  assert.deepEqual(messages, [{ id: "4", data: "hello" }]);
});

test("joins multi-line data and tolerates a missing id", () => {
  const { messages } = parseSse("data: line one\ndata: line two\n\n");
  assert.deepEqual(messages, [{ id: undefined, data: "line one\nline two" }]);
});
