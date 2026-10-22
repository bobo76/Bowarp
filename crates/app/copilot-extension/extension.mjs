import { joinSession } from "@github/copilot-sdk/extension";
import { forwardMode, observeMode } from "./bridge.mjs";

const session = await joinSession({});

if (process.env.AGENTWS_PIPE && process.env.AGENTWS_SESSION && process.env.AGENTWS_HOOK) {
    await observeMode(session, forwardMode, (error) => {
        console.error(`Bowarp mode bridge: ${error.message}`);
    });
}
