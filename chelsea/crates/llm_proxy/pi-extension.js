// Pi extension to route Anthropic requests through the local llm_proxy.
//
// Usage:
//   LLM_PROXY_KEY=sk-vers-... pi --extension ./pi-extension.js
//
// The proxy must be running on localhost:8090 (./dev.sh)

export default function (pi) {
    const proxyUrl = process.env.LLM_PROXY_URL || "http://localhost:8090";
    const proxyKey = process.env.LLM_PROXY_KEY;

    if (!proxyKey) {
        console.error("LLM_PROXY_KEY not set — pi-extension.js won't activate");
        return;
    }

    pi.registerProvider("anthropic", {
        baseUrl: proxyUrl,
        // apiKey accepts an env var name — point it at LLM_PROXY_KEY
        apiKey: "LLM_PROXY_KEY",
    });
}
