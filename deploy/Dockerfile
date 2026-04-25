# Stage 1: Build Rust binaries
# All generated files (validation_rules.rs, expense_report_model.rs,
# ui_field_map.yaml) are embedded into binaries via include_str! / #[path]
# at compile time, so only the compiled binaries are needed at runtime.
FROM rust:1.82-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY generated/ generated/
COPY fixtures/ fixtures/

RUN cargo build --release \
    --bin ingest_bundle_workspace \
    --bin apply_review_revision_to_artifacts \
    --bin render_current_review_surface

# Stage 2: Python runtime with pre-compiled Rust binaries
FROM python:3.12-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends perl && rm -rf /var/lib/apt/lists/*

COPY deploy/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt && rm /tmp/requirements.txt

COPY --from=builder /build/target/release/ingest_bundle_workspace /usr/local/bin/
COPY --from=builder /build/target/release/apply_review_revision_to_artifacts /usr/local/bin/
COPY --from=builder /build/target/release/render_current_review_surface /usr/local/bin/

COPY scripts/ /app/scripts/

WORKDIR /app

RUN mkdir -p /workspace

ENV HOST=0.0.0.0

EXPOSE 8080

CMD ["python3", "scripts/local_app.py", "--workspace-root", "/workspace"]
