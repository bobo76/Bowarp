import assert from "node:assert/strict";
import { test } from "node:test";
import { setImmediate } from "node:timers/promises";
import { forwardMode, observeMode } from "./bridge.mjs";

function fakeSession(getMode) {
    let handler;
    let unsubscribed = false;
    return {
        rpc: { mode: { get: getMode } },
        on(type, listener) {
            assert.equal(type, "session.mode_changed");
            handler = listener;
            return () => { unsubscribed = true; };
        },
        emit(mode, agentId) {
            if (!unsubscribed) {
                handler({ data: { newMode: mode }, agentId });
            }
        },
    };
}

test("reports the initial mode and live changes without observing subagents", async () => {
    const session = fakeSession(async () => "interactive");
    const modes = [];
    const unsubscribe = await observeMode(session, (mode) => modes.push(mode), assert.fail);
    session.emit("plan", "child");
    session.emit("plan");
    session.emit("autopilot");
    await setImmediate();
    assert.deepEqual(modes, ["interactive", "plan", "autopilot"]);
    unsubscribe();
    session.emit("interactive");
    await setImmediate();
    assert.equal(modes.length, 3);
});

test("a live change takes precedence over an in-flight initial snapshot", async () => {
    let resolve;
    const session = fakeSession(() => new Promise((done) => { resolve = done; }));
    const modes = [];
    const ready = observeMode(session, (mode) => modes.push(mode), assert.fail);
    session.emit("autopilot");
    resolve("interactive");
    await ready;
    assert.deepEqual(modes, ["autopilot"]);
});

test("serializes forwarding and recovers after reporting a delivery failure", async () => {
    const session = fakeSession(async () => "interactive");
    const modes = [];
    const errors = [];
    let finish;
    await observeMode(session, async (mode) => {
        modes.push(mode);
        if (mode === "plan") {
            await new Promise((done) => { finish = done; });
            throw new Error("pipe closed");
        }
    }, (error) => errors.push(error.message));
    session.emit("plan");
    session.emit("autopilot");
    await setImmediate();
    assert.deepEqual(modes, ["interactive", "plan"]);
    finish();
    await setImmediate();
    assert.deepEqual(modes, ["interactive", "plan", "autopilot"]);
    assert.deepEqual(errors, ["pipe closed"]);
});

test("reports query failures and still receives later mode changes", async () => {
    const session = fakeSession(async () => { throw new Error("RPC unavailable"); });
    const modes = [];
    const errors = [];
    await observeMode(session, (mode) => modes.push(mode), (error) => errors.push(error.message));
    session.emit("autopilot");
    await setImmediate();
    assert.deepEqual(errors, ["RPC unavailable"]);
    assert.deepEqual(modes, ["autopilot"]);
});

test("rejects unsupported modes instead of reporting a default", async () => {
    const session = fakeSession(async () => "future-mode");
    const modes = [];
    const errors = [];
    await observeMode(session, (mode) => modes.push(mode), (error) => errors.push(error.message));
    assert.deepEqual(modes, []);
    assert.deepEqual(errors, ["Unsupported Copilot mode: future-mode"]);
    await assert.rejects(forwardMode("unknown"), /Unsupported Copilot mode/);
});

test("reports a missing helper as a delivery error", async () => {
    await assert.rejects(
        forwardMode("autopilot", {
            ...process.env,
            AGENTWS_HOOK: "bowarp-nonexistent-test-helper",
        }),
        /ENOENT/,
    );
});
