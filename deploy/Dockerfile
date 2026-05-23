# Stage 1: Build Rust binaries
# All generated files (validation_rules.rs, expense_report_model.rs,
# ui_field_map.yaml) are embedded into binaries via include_str! / #[path]
# at compile time, so only the compiled binaries are needed at runtime.
FROM rust:1.82-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY generated/ generated/

RUN cargo build --release \
    --bin reduce_extractions \
    --bin render_workbench_from_report

# Stage 2: Python runtime with pre-compiled Rust binaries
FROM python:3.12-slim-bookworm

COPY deploy/requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt && rm /tmp/requirements.txt

COPY --from=builder /build/target/release/reduce_extractions /usr/local/bin/
COPY --from=builder /build/target/release/render_workbench_from_report /usr/local/bin/

COPY scripts/ /app/scripts/
# Per-kind extractors read response_schema_<kind>.json from generated/ at runtime;
# the rest of generated/ is already baked into the Rust binaries. Glob lets
# future per-kind schemas ship without a Dockerfile change.
COPY generated/response_schema_*.json /app/generated/
# Stage 18d extracted the upload-form / progress / error HTML strings
# from scripts/local_app_simple.py into dedicated template files.
# local_app_simple.py reads them at module import via
# (_TEMPLATES_DIR / "...").read_text(); without this COPY the
# container crashes on Flask startup with FileNotFoundError.
COPY templates/ /app/templates/

WORKDIR /app

ENV HOST=0.0.0.0 \
    PORT=8080 \
    RUST_BIN_DIR=/usr/local/bin

EXPOSE 8080

# gunicorn loads scripts/local_app_simple.py as the WSGI module.
# --workers 1 matches Cloud Run --max-instances=1; --threads handles
# concurrent uploads on the single instance. --timeout 600 matches
# Cloud Run's request timeout.
CMD ["gunicorn", "--chdir", "/app/scripts", \
     "--bind", "0.0.0.0:8080", \
     "--workers", "1", \
     "--threads", "8", \
     "--timeout", "600", \
     "--access-logfile", "-", \
     "local_app_simple:app"]
