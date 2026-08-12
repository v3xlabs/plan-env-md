default:
    @just --list

# migrations apply automatically when the server starts
setup:
    mkdir -p data
    cd web && pnpm install

# backend with auto-restart (terminal 1)
dev:
    bacon run

# vite dev server proxying /api to the backend (terminal 2)
dev-web:
    cd web && pnpm dev

# refresh committed .sqlx offline query data after query changes
prepare:
    cargo sqlx prepare

# regenerate the OpenAPI spec and the typed frontend schema after API changes
gen-api:
    cargo run --quiet -- spec > web/openapi.json
    cd web && pnpm exec openapi-typescript openapi.json -o src/api/schema.gen.ts

# dump the OpenAPI spec to stdout
spec:
    cargo run --quiet -- spec

fmt:
    cargo fmt
    cd web && pnpm lint:fix

lint:
    cargo clippy --all-targets -- -D warnings
    cd web && pnpm lint && pnpm typecheck

test:
    cargo test

# production build: SPA first, the binary embeds web/dist
build:
    cd web && pnpm build
    cargo build --release

docker:
    docker build -t plan-env-md .
