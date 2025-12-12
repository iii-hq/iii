"""CLI for Motia framework."""

import argparse
import glob
import importlib.util
import logging
import os
import sys
from pathlib import Path

log = logging.getLogger("motia.cli")


def load_module_from_path(path: str) -> None:
    """Load a Python module from a file path."""
    spec = importlib.util.spec_from_file_location("step_module", path)
    if spec and spec.loader:
        module = importlib.util.module_from_spec(spec)
        sys.modules["step_module"] = module
        spec.loader.exec_module(module)


def discover_steps(directory: str) -> list[str]:
    """Discover step files in a directory."""
    if not os.path.exists(directory):
        return []
    return glob.glob(os.path.join(directory, "**", "*.step.py"), recursive=True)


def discover_streams(directory: str) -> list[str]:
    """Discover stream files in a directory."""
    if not os.path.exists(directory):
        return []
    return glob.glob(os.path.join(directory, "**", "*.stream.py"), recursive=True)


def configure_logging(verbose: bool = False) -> None:
    """Configure logging for the CLI."""
    level = logging.DEBUG if verbose else logging.INFO
    format = "%(asctime)s [%(levelname)s] %(name)s: %(message)s"
    logging.basicConfig(level=level, format=format)


def main() -> None:
    """Main entry point for the CLI."""
    parser = argparse.ArgumentParser(description="Motia CLI")
    parser.add_argument(
        "command",
        choices=["run", "build"],
        help="Command to run",
    )
    parser.add_argument(
        "--dir",
        "-d",
        default="steps",
        help="Directory containing step files",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Enable verbose logging",
    )
    parser.add_argument(
        "--watch",
        "-w",
        action="store_true",
        help="Watch for file changes",
    )

    args = parser.parse_args()
    configure_logging(args.verbose)

    if args.command == "run":
        step_files = discover_steps(args.dir)
        stream_files = discover_streams(args.dir)

        log.info(f"Found {len(step_files)} step(s) and {len(stream_files)} stream(s)")

        for stream_file in stream_files:
            log.debug(f"Loading stream: {stream_file}")
            load_module_from_path(stream_file)

        for step_file in step_files:
            log.debug(f"Loading step: {step_file}")
            load_module_from_path(step_file)

        log.info("All steps loaded. Connecting to bridge...")

        try:
            import asyncio

            from .bridge import bridge

            async def run() -> None:
                await bridge.connect()
                log.info("Connected. Waiting for events...")
                while True:
                    await asyncio.sleep(1)

            asyncio.run(run())
        except KeyboardInterrupt:
            log.info("Shutting down...")
    elif args.command == "build":
        log.info("Build command not yet implemented")


if __name__ == "__main__":
    main()
