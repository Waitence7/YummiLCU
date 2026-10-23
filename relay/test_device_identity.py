import base64
import hashlib
import uuid
import unittest
import time

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

from relay.app import (
    _device_trusted_redis_key,
    _mask_ip,
    _trusted_proxy_client_ip,
    _session_auth_source_redis_key,
    _session_device_fingerprint_redis_key,
    _session_trust_level,
    _verify_device_proof_payload,
)


class FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, str] = {}

    async def get(self, key: str):
        return self.values.get(key)

    async def set(self, key: str, value, **_kwargs):
        self.values[key] = str(value)
        return True

    async def exists(self, key: str) -> int:
        return int(key in self.values)


class DeviceProofTests(unittest.TestCase):
    def test_valid_device_proof_binds_public_key_to_challenge(self) -> None:
        private_key = Ed25519PrivateKey.generate()
        public_key = private_key.public_key().public_bytes(
            serialization.Encoding.Raw,
            serialization.PublicFormat.Raw,
        )
        public_key_b64 = base64.b64encode(public_key).decode("ascii")
        session_id = str(uuid.uuid4())
        connection_id = str(uuid.uuid4())
        challenge_id = str(uuid.uuid4())
        device_id = str(uuid.uuid4())
        nonce = "n" * 43
        message = (
            "yummi-device-proof-v1\0"
            f"{challenge_id}\0{nonce}\0{session_id}\0{connection_id}\0"
            f"{device_id}\0{public_key_b64}"
        ).encode("utf-8")
        signature = base64.b64encode(private_key.sign(message)).decode("ascii")
        challenge = {
            "challenge_id": challenge_id,
            "nonce": nonce,
            "device_id": device_id,
            "public_key": public_key_b64,
            "issued_at_ms": str(time.time_ns() // 1_000_000),
        }
        proof = {
            "challenge_id": challenge_id,
            "device_id": device_id,
            "device_public_key": public_key_b64,
            "signature": signature,
        }

        result = _verify_device_proof_payload(
            challenge,
            proof,
            session_id=session_id,
            connection_id=connection_id,
        )

        self.assertEqual(result, (hashlib.sha256(public_key).hexdigest(), device_id))

    def test_replayed_signature_for_other_connection_is_rejected(self) -> None:
        private_key = Ed25519PrivateKey.generate()
        public_key = private_key.public_key().public_bytes(
            serialization.Encoding.Raw,
            serialization.PublicFormat.Raw,
        )
        public_key_b64 = base64.b64encode(public_key).decode("ascii")
        session_id = str(uuid.uuid4())
        signed_connection_id = str(uuid.uuid4())
        other_connection_id = str(uuid.uuid4())
        challenge_id = str(uuid.uuid4())
        device_id = str(uuid.uuid4())
        nonce = "x" * 43
        message = (
            "yummi-device-proof-v1\0"
            f"{challenge_id}\0{nonce}\0{session_id}\0{signed_connection_id}\0"
            f"{device_id}\0{public_key_b64}"
        ).encode("utf-8")
        proof = {
            "challenge_id": challenge_id,
            "device_id": device_id,
            "device_public_key": public_key_b64,
            "signature": base64.b64encode(private_key.sign(message)).decode("ascii"),
        }
        challenge = {
            "challenge_id": challenge_id,
            "nonce": nonce,
            "device_id": device_id,
            "public_key": public_key_b64,
            "issued_at_ms": str(time.time_ns() // 1_000_000),
        }

        self.assertIsNone(
            _verify_device_proof_payload(
                challenge,
                proof,
                session_id=session_id,
                connection_id=other_connection_id,
            )
        )


class DeviceTrustTests(unittest.IsolatedAsyncioTestCase):
    async def test_trust_follows_fingerprint_not_session_id(self) -> None:
        redis = FakeRedis()
        first_session = str(uuid.uuid4())
        second_session = str(uuid.uuid4())
        fingerprint = "a" * 64
        discord_id = 42
        redis.values[_session_device_fingerprint_redis_key(first_session)] = fingerprint
        redis.values[_session_device_fingerprint_redis_key(second_session)] = fingerprint
        redis.values[_session_auth_source_redis_key(first_session)] = "auto"
        redis.values[_session_auth_source_redis_key(second_session)] = "auto"

        self.assertEqual(
            await _session_trust_level(redis, discord_id, first_session),
            "new_device",
        )
        redis.values[_device_trusted_redis_key(discord_id, fingerprint)] = "1"
        self.assertEqual(
            await _session_trust_level(redis, discord_id, second_session),
            "trusted",
        )


class ClientIpTests(unittest.TestCase):
    class _Client:
        def __init__(self, host: str) -> None:
            self.host = host

    class _Ws:
        def __init__(self, peer: str, headers: dict[str, str]) -> None:
            self.client = ClientIpTests._Client(peer)
            self.headers = headers

    def test_trusted_private_proxy_uses_forwarded_public_ip(self) -> None:
        ws = self._Ws("172.18.0.4", {"x-forwarded-for": "8.8.4.4, 172.18.0.4"})
        self.assertEqual(_trusted_proxy_client_ip(ws), "8.8.4.4")

    def test_public_peer_cannot_spoof_forwarded_ip(self) -> None:
        ws = self._Ws("8.8.8.8", {"x-forwarded-for": "1.1.1.1"})
        self.assertEqual(_trusted_proxy_client_ip(ws), "8.8.8.8")

    def test_ip_masking_keeps_only_coarse_network(self) -> None:
        self.assertEqual(_mask_ip("8.8.8.8"), "8.8.8.0/24")
        self.assertEqual(_mask_ip("2001:4860:4860::8888"), "2001:4860:4860::/56")


if __name__ == "__main__":
    unittest.main()
