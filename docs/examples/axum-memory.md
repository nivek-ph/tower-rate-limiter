# Axum with nested policies

This example installs a global policy around the application and a stricter policy around the auth
routes. It also shows the `ConnectInfo<SocketAddr>` setup required by `IpKeyExtractor` and an IP
allowlist that bypasses both policies.

```mermaid
%%{init: {"themeVariables": {"fontSize": "10px"}, "flowchart": {"curve": "basis", "useMaxWidth": false, "padding": 5, "nodeSpacing": 15, "rankSpacing": 20}}}%%
flowchart LR
    request["Request"] --> allowlisted{"Peer allowlisted?<br/>ConnectInfo IP"}
    allowlisted -- "Yes" --> bypass["Bypass both Layers"]
    bypass --> bypass_response["Handler response<br/>No rate-limit fields"]
    allowlisted -- "No" --> global["global-limit<br/>Charges every route"]
    global --> auth_route{"Under /auth?"}
    auth_route -- "No" --> other["Other handler"]
    other --> global_response["Handler response<br/>Global field only"]
    auth_route -- "Yes" --> auth["auth-limit<br/>Charges auth routes"]
    auth --> auth_handler["Auth handler"]
    auth_handler --> auth_response["Handler response<br/>Auth and global fields"]

    classDef entry fill:#ede9fe,stroke:#8b5cf6,color:#3b0764,stroke-width:2px
    classDef decision fill:#fef3c7,stroke:#f59e0b,color:#78350f,stroke-width:1.5px
    classDef policy fill:#dbeafe,stroke:#3b82f6,color:#172554,stroke-width:1.5px
    classDef success fill:#dcfce7,stroke:#22c55e,color:#14532d,stroke-width:1.5px
    classDef bypass fill:#f1f5f9,stroke:#64748b,color:#1e293b,stroke-width:1.5px

    class request entry
    class allowlisted,auth_route decision
    class global,auth policy
    class global_response,auth_response success
    class bypass,bypass_response,other,auth_handler bypass
```

```sh
cargo run --example axum_memory --features axum,memory
```

The server listens on `http://127.0.0.1:3000`.

## Complete source

```rust,ignore
{{#include ../../examples/axum_memory.rs}}
```

An allowlisted request reaches the handler without quota metadata. A non-allowlisted request to
`/auth/login` can consume both policies; if the inner auth policy rejects it, the already-recorded
outer charge is not refunded.
