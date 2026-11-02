from relay.connections import ConnectionManager


def test_party_lobby_cache_returns_last_pushed_state_without_agent_command():
    manager = ConnectionManager()
    manager.set_party_lobby(
        123,
        {
            "in_lobby": True,
            "riot_ids_in_party": ["Alpha#KR1", "Beta#KR2"],
        },
    )

    row = manager.get_party_lobby(123)

    assert row is not None
    assert row["data"] == {
        "in_lobby": True,
        "riot_ids_in_party": ["Alpha#KR1", "Beta#KR2"],
    }
    assert isinstance(row["age_ms"], int)
    assert row["age_ms"] >= 0


def test_party_lobby_cache_is_empty_for_unknown_agent():
    manager = ConnectionManager()
    assert manager.get_party_lobby(999) is None
