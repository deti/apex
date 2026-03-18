const { add, classify } = require("./index");
const assert = require("node:assert");
const { test } = require("node:test");

test("add positive numbers", () => {
  assert.strictEqual(add(1, 2), 3);
});

test("classify positive", () => {
  assert.strictEqual(classify(5), "positive");
});
