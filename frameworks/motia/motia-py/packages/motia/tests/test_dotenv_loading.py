"""Test that .env file is loaded at CLI startup."""

from unittest.mock import patch, MagicMock

from motia.cli import main, generate_index


def test_main_loads_dotenv() -> None:
    """main() should call load_dotenv() before doing anything else."""
    with patch("motia.cli.load_dotenv") as mock_load:
        # Make argparse raise SystemExit so main() stops early
        with patch("sys.argv", ["motia"]):
            try:
                main()
            except SystemExit:
                pass
        mock_load.assert_called_once()


def test_generated_index_includes_dotenv() -> None:
    """The generated index.py should include dotenv loading."""
    result = generate_index([], [])
    assert "from dotenv import load_dotenv" in result
    assert "load_dotenv()" in result
