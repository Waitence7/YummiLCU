# Yummi LiveKit SFU

The Agent publishes one 720p/15fps H.264 WebRTC track to this SFU. Browsers subscribe through LiveKit, so adding viewers does not increase Agent upload bandwidth.

1. Copy `livekit.yaml.example` to `livekit.yaml` and set a random API key/secret.
2. Set Relay `LIVEKIT_PUBLIC_URL=wss://<live-domain>`, `LIVEKIT_API_KEY`, and `LIVEKIT_API_SECRET` to the same credentials.
3. Terminate TLS for port 7880 behind the live domain. Expose TCP 7881 and UDP 50000-60000 to clients.
4. Start with `docker compose -f livekit/docker-compose.yml up -d` from the repository root.

`livekit.yaml` is intentionally ignored and must never be committed. Pin `LIVEKIT_IMAGE` to a tested server release in production.
