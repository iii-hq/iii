"""The iii.channel submodule exposes the channel types; the root no longer does."""


def test_channel_subpath() -> None:
    from iii.channel import Channel, ChannelReader, ChannelWriter, StreamChannelRef

    assert all(x is not None for x in (ChannelReader, ChannelWriter, StreamChannelRef, Channel))


def test_channel_types_not_at_root() -> None:
    import iii

    for name in ("Channel", "ChannelReader", "ChannelWriter", "StreamChannelRef"):
        assert not hasattr(iii, name)
