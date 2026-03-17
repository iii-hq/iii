import json
import sys
from pathlib import Path
from types import ModuleType, SimpleNamespace
from unittest.mock import MagicMock

import pytest

from motia import cli


def test_load_module_from_path_loads_python_module(tmp_path: Path) -> None:
    module_path = tmp_path / "sample_module.py"
    module_path.write_text("VALUE = 42\n")

    module = cli.load_module_from_path(str(module_path))

    assert module is not None
    assert module.VALUE == 42


def test_load_module_from_path_returns_none_without_loader(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(cli.importlib.util, "spec_from_file_location", lambda name, path: SimpleNamespace(loader=None))

    assert cli.load_module_from_path("missing.py") is None


def test_load_and_register_step_registers_valid_step(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    step_path = tmp_path / "valid_step.py"
    step_path.write_text(
        "config = {'name': 'demo', 'triggers': [{'type': 'queue', 'topic': 'demo.created'}]}\n"
        "async def handler(data, ctx):\n"
        "    return data\n"
    )

    fake_motia = SimpleNamespace(add_step=MagicMock())
    monkeypatch.setattr("motia.runtime.Motia", lambda: fake_motia)

    cli.load_and_register_step(str(step_path))

    fake_motia.add_step.assert_called_once()
    args = fake_motia.add_step.call_args[0]
    assert args[0]["name"] == "demo"
    assert args[1] == str(step_path)
    assert callable(args[2])


def test_load_and_register_step_ignores_missing_module(monkeypatch: pytest.MonkeyPatch) -> None:
    fake_motia = SimpleNamespace(add_step=MagicMock())
    monkeypatch.setattr("motia.runtime.Motia", lambda: fake_motia)
    monkeypatch.setattr(cli, "load_module_from_path", lambda path: None)

    cli.load_and_register_step("missing_step.py")

    fake_motia.add_step.assert_not_called()


def test_discover_helpers_find_steps_and_streams(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    (tmp_path / "steps").mkdir()
    (tmp_path / "src").mkdir()
    (tmp_path / "steps" / "alpha_step.py").write_text("")
    (tmp_path / "src" / "beta_step.py").write_text("")
    (tmp_path / "steps" / "gamma_stream.py").write_text("")
    (tmp_path / "src" / "delta_stream.py").write_text("")

    monkeypatch.chdir(tmp_path)

    discovered = cli._discover_files(["steps", "missing"], "*_step.py")
    steps = cli.discover_steps("steps", include_src=True)
    streams = cli.discover_streams("steps", include_src=True)

    def _norm(paths: list[str]) -> list[str]:
        return sorted(str(Path(p)) for p in paths)

    assert _norm(discovered) == _norm(["steps/alpha_step.py"])
    assert _norm(steps) == _norm(["steps/alpha_step.py", "src/beta_step.py"])
    assert _norm(streams) == _norm(["steps/gamma_stream.py", "src/delta_stream.py"])


def test_configure_logging_uses_expected_level(monkeypatch: pytest.MonkeyPatch) -> None:
    basic_config = MagicMock()
    monkeypatch.setattr(cli.logging, "basicConfig", basic_config)

    cli.configure_logging(verbose=True)

    basic_config.assert_called_once()
    assert basic_config.call_args.kwargs["level"] == cli.logging.DEBUG


def test_generate_index_lists_streams_before_steps() -> None:
    result = cli.generate_index(["steps/example_step.py"], ["streams/example_stream.py"])

    assert "load_module_from_path(r'streams/example_stream.py')" in result
    assert "load_and_register_step(r'steps/example_step.py')" in result
    assert result.index("load_module_from_path(r'streams/example_stream.py')") < result.index(
        "load_and_register_step(r'steps/example_step.py')"
    )


def test_generate_schema_manifest_supports_model_dump_and_fallback(monkeypatch: pytest.MonkeyPatch) -> None:
    step_path = "steps/example_step.py"
    stream_path = "streams/example_stream.py"

    class ConfigWithDump:
        def model_dump(self, by_alias: bool = True) -> dict[str, str]:
            return {"name": "step"}

    class ConfigWithBrokenDump:
        def model_dump(self, by_alias: bool = True) -> dict[str, str]:
            raise RuntimeError("boom")

    def fake_loader(path: str) -> ModuleType:
        module = ModuleType(f"step_module:{path}")
        if path == step_path:
            module.config = ConfigWithDump()
        else:
            module.config = ConfigWithBrokenDump()
        monkeypatch.setitem(sys.modules, f"step_module:{path}", module)
        return module

    monkeypatch.setattr(cli, "load_module_from_path", fake_loader)

    manifest = cli.generate_schema_manifest([step_path], [stream_path])

    assert manifest == {
        "steps": [{"filePath": step_path, "config": {"name": "step"}}],
        "streams": [{"filePath": stream_path, "config": sys.modules[f"step_module:{stream_path}"].config}],
    }


def test_main_build_writes_dist_index(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(cli, "discover_steps", lambda directory, include_src=True: ["steps/demo_step.py"])
    monkeypatch.setattr(cli, "discover_streams", lambda directory, include_src=True: ["streams/demo_stream.py"])
    monkeypatch.setattr(cli, "generate_index", lambda steps, streams: "print('built')\n")
    monkeypatch.setattr(sys, "argv", ["motia", "build"])

    cli.main()

    assert (tmp_path / "dist" / "index.py").read_text() == "print('built')\n"


def test_main_typegen_writes_manifest(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(cli, "discover_steps", lambda directory, include_src=True: ["steps/demo_step.py"])
    monkeypatch.setattr(cli, "discover_streams", lambda directory, include_src=True: ["streams/demo_stream.py"])
    monkeypatch.setattr(
        cli,
        "generate_schema_manifest",
        lambda steps, streams: {"steps": [{"filePath": steps[0]}], "streams": [{"filePath": streams[0]}]},
    )
    monkeypatch.setattr(sys, "argv", ["motia", "typegen", "--output", "manifest.json"])

    cli.main()

    assert json.loads((tmp_path / "manifest.json").read_text()) == {
        "steps": [{"filePath": "steps/demo_step.py"}],
        "streams": [{"filePath": "streams/demo_stream.py"}],
    }


def test_main_run_loads_modules_and_handles_keyboard_interrupt(monkeypatch: pytest.MonkeyPatch) -> None:
    loaded_streams: list[str] = []
    loaded_steps: list[str] = []

    monkeypatch.setattr(cli, "discover_steps", lambda directory, include_src=True: ["steps/demo_step.py"])
    monkeypatch.setattr(cli, "discover_streams", lambda directory, include_src=True: ["streams/demo_stream.py"])
    monkeypatch.setattr(cli, "load_module_from_path", loaded_streams.append)
    monkeypatch.setattr(cli, "load_and_register_step", loaded_steps.append)
    monkeypatch.setattr("motia.iii.get_instance", lambda: SimpleNamespace(_wait_until_connected=MagicMock(), shutdown=MagicMock()))
    monkeypatch.setattr(cli.threading, "Event", lambda: SimpleNamespace(wait=MagicMock(), set=MagicMock()))
    monkeypatch.setattr(sys, "argv", ["motia", "run"])

    cli.main()

    assert loaded_streams == ["streams/demo_stream.py"]
    assert loaded_steps == ["steps/demo_step.py"]


def test_main_dev_watch_restarts_process(monkeypatch: pytest.MonkeyPatch) -> None:
    watchfiles = ModuleType("watchfiles")
    watchfiles.watch = lambda *paths: iter([("changed", "steps/demo_step.py")])
    monkeypatch.setitem(sys.modules, "watchfiles", watchfiles)
    monkeypatch.setattr(cli, "discover_steps", lambda directory, include_src=True: ["steps/demo_step.py"])
    monkeypatch.setattr(cli, "discover_streams", lambda directory, include_src=True: ["streams/demo_stream.py"])
    monkeypatch.setattr(sys, "argv", ["motia", "dev", "--watch"])

    def fake_execv(path: str, argv: list[str]) -> None:
        raise KeyboardInterrupt

    monkeypatch.setattr(cli.os, "execv", fake_execv)

    with pytest.raises(KeyboardInterrupt):
        cli.main()


def test_main_dev_without_watchfiles_logs_warning_and_handles_keyboard_interrupt(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    loaded_streams: list[str] = []
    loaded_steps: list[str] = []
    warnings: list[str] = []

    real_import = __import__

    def fake_import(name: str, *args: object, **kwargs: object):
        if name == "watchfiles":
            raise ImportError("missing")
        return real_import(name, *args, **kwargs)

    monkeypatch.setattr("builtins.__import__", fake_import)
    monkeypatch.setattr(cli, "discover_steps", lambda directory, include_src=True: ["steps/demo_step.py"])
    monkeypatch.setattr(cli, "discover_streams", lambda directory, include_src=True: ["streams/demo_stream.py"])
    monkeypatch.setattr(cli, "load_module_from_path", loaded_streams.append)
    monkeypatch.setattr(cli, "load_and_register_step", loaded_steps.append)
    monkeypatch.setattr("motia.iii.get_instance", lambda: SimpleNamespace(_wait_until_connected=MagicMock(), shutdown=MagicMock()))
    monkeypatch.setattr(cli.log, "warning", lambda message: warnings.append(message))
    monkeypatch.setattr(cli.threading, "Event", lambda: SimpleNamespace(wait=MagicMock(), set=MagicMock()))
    monkeypatch.setattr(sys, "argv", ["motia", "dev", "--watch"])

    cli.main()

    assert warnings == ["watchfiles not available; running without watch mode"]
    assert loaded_streams == ["streams/demo_stream.py"]
    assert loaded_steps == ["steps/demo_step.py"]
