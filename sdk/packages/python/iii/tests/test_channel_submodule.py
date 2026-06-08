"""The iii.channel submodule exposes the channel types; the root keeps the shims."""


def test_channel_subpath() -> None:
    from iii.channel import Channel, ChannelReader, ChannelWriter, StreamChannelRef

    assert all(x is not None for x in (ChannelReader, ChannelWriter, StreamChannelRef, Channel))


def test_channel_root_shim() -> None:
    import iii
    from iii.channel import ChannelReader as SubReader

    assert iii.ChannelReader is SubReader
