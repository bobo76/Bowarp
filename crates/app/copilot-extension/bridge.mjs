import { execFile } from "node:child_process";

const MODES = new Set(["interactive", "plan", "autopilot"]);
const HOOK_TIMEOUT_MS = 5000;

export function forwardMode(mode, env = process.env) {
    if (!MODES.has(mode)) {
        return Promise.reject(new Error(`Unsupported Copilot mode: ${mode}`));
    }
    return new Promise((resolve, reject) => {
        const child = execFile(env.AGENTWS_HOOK, ["--report-errors"], {
            env,
            windowsHide: true,
            timeout: HOOK_TIMEOUT_MS,
        }, (error) => error ? reject(error) : resolve());
        child.stdin.on("error", reject);
        child.stdin.end(JSON.stringify({
            hook_event_name: "CopilotMode",
            notification_type: mode,
        }));
    });
}

export async function observeMode(session, forward, onError) {
    let changed = false;
    let pending = Promise.resolve();
    const report = (mode) => {
        if (!MODES.has(mode)) {
            onError(new Error(`Unsupported Copilot mode: ${mode}`));
            return;
        }
        // Keep helper processes ordered so a slow earlier report cannot win.
        pending = pending.then(() => forward(mode)).catch(onError);
    };
    const unsubscribe = session.on("session.mode_changed", (event) => {
        if (event.agentId) {
            return;
        }
        changed = true;
        report(event.data.newMode);
    });
    try {
        const mode = await session.rpc.mode.get();
        // A live event received during the initial query is newer than its snapshot.
        if (!changed) {
            report(mode);
        }
    } catch (error) {
        onError(error);
    }
    await pending;
    return unsubscribe;
}
