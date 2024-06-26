FROM rust:1.77.2-buster as builder
RUN apt-get update && apt-get install -y --no-install-recommends cmake musl-tools wget && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /afrotd
RUN wget -nv https://afsvd.de/content/files/2023/12/Football_Regelbuch_2024.pdf
COPY . .

RUN cargo build --target=x86_64-unknown-linux-musl --release

FROM alpine:3.19
RUN apk add --no-cache poppler-utils
WORKDIR /afrotd
COPY --from=builder /afrotd/target/x86_64-unknown-linux-musl/release/afrotd /afrotd/afrotd
COPY --from=builder /afrotd/Football_Regelbuch_2024.pdf /afrotd/Football_Regelbuch_2024.pdf
COPY res ./res
EXPOSE 3000
ENTRYPOINT ["/afrotd/afrotd"]
