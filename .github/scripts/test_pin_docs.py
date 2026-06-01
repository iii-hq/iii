"""Unit tests for pin_docs.py.

Run with: python -m pytest .github/scripts/test_pin_docs.py -v
"""
from __future__ import annotations

import json
from pathlib import Path

import pytest

from pin_docs import add_prefix
from pin_docs import copy_root_to_dir
from pin_docs import dir_prefix
from pin_docs import find_index_by_tag
from pin_docs import is_excluded
from pin_docs import minor_label
from pin_docs import next_minor
from pin_docs import parse_version
from pin_docs import replace_root_with_dir
from pin_docs import rotate
from pin_docs import sort_versions
from pin_docs import strip_prefix
from pin_docs import sync_patch
from pin_docs import validate


class TestParseVersion:
    def test_stable(self):
        assert parse_version("0.17.0") == (0, 17, 0)

    def test_prerelease_suffix_ignored(self):
        assert parse_version("1.0.0-next.5") == (1, 0, 0)

    def test_dry_run_suffix_ignored(self):
        assert parse_version("0.17.0-dry-run.2") == (0, 17, 0)

    def test_rejects_garbage(self):
        with pytest.raises(ValueError):
            parse_version("nope")


class TestLabelPrefix:
    def test_minor_label(self):
        assert minor_label(0, 17) == "0.17.x"
        assert minor_label(1, 0) == "1.0.x"

    def test_dir_prefix(self):
        assert dir_prefix(0, 17) == "0-17-0"
        assert dir_prefix(1, 0) == "1-0-0"

    def test_next_minor(self):
        assert next_minor("0.17.0") == (0, 18)
        assert next_minor("1.0.0") == (1, 1)
        assert next_minor("0.17.0-next.1") == (0, 18)


def _tabs(*pages):
    return [{"tab": "Docs", "groups": [
        {"group": "G", "pages": list(pages)},
        {"group": "Nested", "pages": [{"group": "Sub", "pages": ["how-to/a"]}]},
    ]}]


class TestAddStripPrefix:
    def test_add_prefix(self):
        out = add_prefix(_tabs("index", "install"), "next")
        assert out[0]["groups"][0]["pages"] == ["next/index", "next/install"]
        assert out[0]["groups"][1]["pages"][0]["pages"] == ["next/how-to/a"]

    def test_strip_prefix(self):
        prefixed = add_prefix(_tabs("index"), "next")
        out = strip_prefix(prefixed, "next")
        assert out[0]["groups"][0]["pages"] == ["index"]
        assert out[0]["groups"][1]["pages"][0]["pages"] == ["how-to/a"]

    def test_add_does_not_mutate_input(self):
        tabs = _tabs("index")
        add_prefix(tabs, "next")
        assert tabs[0]["groups"][0]["pages"] == ["index"]

    def test_strip_only_removes_matching(self):
        out = strip_prefix(_tabs("index"), "next")
        assert out[0]["groups"][0]["pages"] == ["index"]

    def test_add_prefix_keeps_shared_changelog_at_root(self):
        tabs = [{"tab": "D", "groups": [{"group": "G", "pages": [
            "using-iii/workers", "changelog/index", "changelog/0-11-0/x",
        ]}]}]
        out = add_prefix(tabs, "0-16-0")
        assert out[0]["groups"][0]["pages"] == [
            "0-16-0/using-iii/workers", "changelog/index", "changelog/0-11-0/x",
        ]


class TestFindIndexByTag:
    def test_finds(self):
        versions = [{"tag": "Latest"}, {"tag": "Next"}, {}]
        assert find_index_by_tag(versions, "Next") == 1
        assert find_index_by_tag(versions, "Latest") == 0

    def test_raises_when_absent(self):
        with pytest.raises(ValueError):
            find_index_by_tag([{"tag": "Latest"}], "Next")


class TestSortVersions:
    def test_order(self):
        versions = [
            {"version": "0.12.x"},
            {"version": "0.16.x", "tag": "Latest", "default": True},
            {"version": "0.10.x"},
            {"version": "0.17.x", "tag": "Next"},
        ]
        out = sort_versions(versions)
        assert [v["version"] for v in out] == ["0.17.x", "0.16.x", "0.12.x", "0.10.x"]
        assert out[0]["tag"] == "Next"
        assert out[1]["tag"] == "Latest"

    def test_double_digit_numeric(self):
        out = sort_versions([{"version": "0.2.x"}, {"version": "0.10.x"}])
        assert [v["version"] for v in out] == ["0.10.x", "0.2.x"]


class TestIsExcluded:
    @pytest.mark.parametrize("name", [
        "docs.json", "package.json", "node_modules", "custom.css",
        "navbar-counters.js", ".gitignore", ".mintignore", ".prettierrc",
        "README.md", "0-11-0", "1-0-0", "index.mdx.skill.md", "next", "changelog",
    ])
    def test_excluded(self, name):
        assert is_excluded(name) is True

    @pytest.mark.parametrize("name", [
        "index.mdx", "using-iii", "assets", "images",
    ])
    def test_included(self, name):
        assert is_excluded(name) is False


def _make_root(tmp_path: Path) -> Path:
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.mdx").write_text("# Home latest")
    (docs / "index.mdx.skill.md").write_text("sidecar")
    (docs / "docs.json").write_text("{}")
    (docs / ".gitignore").write_text("node_modules/")
    (docs / "using-iii").mkdir()
    (docs / "using-iii" / "workers.mdx").write_text("# Workers")
    return docs


class TestCopyRootToDir:
    def test_copies_content_excludes_infra(self, tmp_path):
        docs = _make_root(tmp_path)
        copy_root_to_dir(docs, "0-16-0")
        pinned = docs / "0-16-0"
        assert (pinned / "index.mdx").read_text() == "# Home latest"
        assert (pinned / "using-iii" / "workers.mdx").exists()
        assert not (pinned / "docs.json").exists()
        assert not (pinned / ".gitignore").exists()
        assert not (pinned / "index.mdx.skill.md").exists()

    def test_does_not_copy_next_folder(self, tmp_path):
        docs = _make_root(tmp_path)
        (docs / "next").mkdir()
        (docs / "next" / "index.mdx").write_text("# Next")
        copy_root_to_dir(docs, "0-16-0")
        assert not (docs / "0-16-0" / "next").exists()

    def test_overwrites_existing(self, tmp_path):
        docs = _make_root(tmp_path)
        (docs / "0-16-0").mkdir()
        (docs / "0-16-0" / "stale.mdx").write_text("old")
        copy_root_to_dir(docs, "0-16-0")
        assert not (docs / "0-16-0" / "stale.mdx").exists()


class TestReplaceRootWithDir:
    def test_replaces_content_keeps_infra_and_next(self, tmp_path):
        docs = _make_root(tmp_path)
        nxt = docs / "next"
        nxt.mkdir()
        (nxt / "index.mdx").write_text("# Home next")
        (nxt / "newpage.mdx").write_text("# New")
        replace_root_with_dir(docs, "next")
        assert (docs / "index.mdx").read_text() == "# Home next"
        assert (docs / "newpage.mdx").exists()
        assert not (docs / "using-iii").exists()
        assert (docs / "docs.json").exists()
        assert (docs / ".gitignore").exists()
        assert (docs / "next" / "index.mdx").exists()


def _write_docs(docs: Path, versions):
    (docs / "docs.json").write_text(json.dumps({"navigation": {"versions": versions}}, indent=2))


def _ready_docs(tmp_path: Path) -> Path:
    """root = Latest 0.16.x; docs/next/ = Next (label 0.17.x)."""
    docs = tmp_path / "docs"
    docs.mkdir()
    (docs / "index.mdx").write_text("# Latest 0.16")
    nxt = docs / "next"
    nxt.mkdir()
    (nxt / "index.mdx").write_text("# Next")
    _write_docs(docs, [
        {"version": "0.16.x", "tag": "Latest", "default": True,
         "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["index"]}]}]},
        {"version": "0.17.x", "tag": "Next",
         "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["next/index"]}]}]},
    ])
    return docs


class TestValidate:
    def test_ok_when_next_block_and_folder_present(self, tmp_path):
        assert validate(_ready_docs(tmp_path)) == 0

    def test_fails_when_folder_missing(self, tmp_path):
        docs = _ready_docs(tmp_path)
        import shutil as _sh
        _sh.rmtree(docs / "next")
        assert validate(docs) == 1

    def test_fails_when_folder_empty(self, tmp_path):
        docs = _ready_docs(tmp_path)
        import shutil as _sh
        _sh.rmtree(docs / "next")
        (docs / "next").mkdir()
        assert validate(docs) == 1

    def test_fails_when_no_next_block(self, tmp_path):
        docs = tmp_path / "docs"
        docs.mkdir()
        (docs / "next").mkdir()
        (docs / "next" / "index.mdx").write_text("x")
        _write_docs(docs, [{"version": "0.16.x", "tag": "Latest"}])
        assert validate(docs) == 1


def _read_versions(docs: Path):
    return json.loads((docs / "docs.json").read_text())["navigation"]["versions"]


class TestRotate:
    def test_rotation_0_17_0(self, tmp_path):
        docs = _ready_docs(tmp_path)  # root=Latest 0.16.x; next/=Next 0.17.x
        rotate(docs, "0.17.0")

        assert (docs / "0-16-0" / "index.mdx").read_text() == "# Latest 0.16"
        assert (docs / "index.mdx").read_text() == "# Next"
        assert (docs / "next" / "index.mdx").read_text() == "# Next"

        versions = _read_versions(docs)
        by_tag = {v.get("tag"): v for v in versions}
        assert by_tag["Latest"]["version"] == "0.17.x"
        assert by_tag["Latest"]["tabs"][0]["groups"][0]["pages"] == ["index"]
        assert by_tag["Next"]["version"] == "0.18.x"
        assert by_tag["Next"]["tabs"][0]["groups"][0]["pages"] == ["next/index"]
        archived = [v for v in versions if v.get("tag") not in ("Latest", "Next")]
        old = [v for v in archived if v["version"] == "0.16.x"][0]
        assert old["tabs"][0]["groups"][0]["pages"] == ["0-16-0/index"]

        assert versions[0]["tag"] == "Next"
        assert versions[1]["tag"] == "Latest"
        assert sum(1 for v in versions if v.get("default")) == 1

    def test_changelog_stays_shared_at_root(self, tmp_path):
        docs = tmp_path / "docs"
        docs.mkdir()
        (docs / "index.mdx").write_text("# Latest 0.16")
        (docs / "changelog").mkdir()
        (docs / "changelog" / "index.mdx").write_text("# changelog")
        nxt = docs / "next"
        nxt.mkdir()
        (nxt / "index.mdx").write_text("# Next")
        _write_docs(docs, [
            {"version": "0.16.x", "tag": "Latest", "default": True,
             "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["index"]}]},
                      {"tab": "Changelog", "groups": [{"group": "C", "pages": ["changelog/index"]}]}]},
            {"version": "0.17.x", "tag": "Next",
             "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["next/index"]}]},
                      {"tab": "Changelog", "groups": [{"group": "C", "pages": ["changelog/index"]}]}]},
        ])
        rotate(docs, "0.17.0")
        # Changelog folder never copied into the archive.
        assert not (docs / "0-16-0" / "changelog").exists()
        # Shared changelog still at the root.
        assert (docs / "changelog" / "index.mdx").exists()
        # Every block's changelog tab points at the root changelog.
        for block in _read_versions(docs):
            cl = [t for t in block["tabs"] if t.get("tab") == "Changelog"][0]
            assert cl["groups"][0]["pages"] == ["changelog/index"]

    def test_patch_version_delegates_to_sync(self, tmp_path):
        docs = _ready_docs(tmp_path)  # root=Latest 0.16.x; next/=Next 0.17.x
        before = (docs / "docs.json").read_text()
        rotate(docs, "0.16.1")  # patch -> sync, not rotation
        # No archive folder, docs.json version blocks unchanged.
        assert not (docs / "0-16-0").exists()
        assert (docs / "docs.json").read_text() == before
        # Root content refreshed from next/.
        assert (docs / "index.mdx").read_text() == "# Next"

    def test_rerun_same_version_is_idempotent(self, tmp_path):
        docs = _ready_docs(tmp_path)
        rotate(docs, "0.17.0")
        after_first = (docs / "docs.json").read_text()
        rotate(docs, "0.17.0")  # re-run must be a no-op
        assert (docs / "docs.json").read_text() == after_first
        versions = _read_versions(docs)
        assert sum(1 for v in versions if v["version"] == "0.17.x") == 1
        assert sum(1 for v in versions if v.get("tag") == "Latest") == 1

    def test_major_release_uses_tag_and_minor_plus_one(self, tmp_path):
        docs = tmp_path / "docs"
        docs.mkdir()
        (docs / "index.mdx").write_text("# Latest 0.17")
        nxt = docs / "next"
        nxt.mkdir()
        (nxt / "index.mdx").write_text("# Next 0.18")
        _write_docs(docs, [
            {"version": "0.17.x", "tag": "Latest", "default": True,
             "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["index"]}]}]},
            {"version": "0.18.x", "tag": "Next",
             "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["next/index"]}]}]},
        ])
        rotate(docs, "1.0.0")
        assert (docs / "0-17-0" / "index.mdx").exists()
        assert (docs / "index.mdx").read_text() == "# Next 0.18"
        by_tag = {v.get("tag"): v for v in _read_versions(docs)}
        assert by_tag["Latest"]["version"] == "1.0.x"
        assert by_tag["Next"]["version"] == "1.1.x"
        assert by_tag["Next"]["tabs"][0]["groups"][0]["pages"] == ["next/index"]

    def test_rewrites_internal_links_on_move(self, tmp_path):
        docs = tmp_path / "docs"
        docs.mkdir()
        # Latest (root, 0.16) — root-absolute links + an asset (img src).
        (docs / "using-iii").mkdir()
        (docs / "using-iii" / "workers.mdx").write_text("# root workers")
        (docs / "assets").mkdir()
        (docs / "assets" / "pic.png").write_text("png")
        (docs / "index.mdx").write_text(
            'See [workers](/using-iii/workers). <img src="/assets/pic.png" />'
        )
        # Next (next/, 0.17) — next/-prefixed links + a legacy cross-version ref + asset.
        nxt = docs / "next"
        (nxt / "using-iii").mkdir(parents=True)
        (nxt / "using-iii" / "workers.mdx").write_text("# next workers")
        (nxt / "assets").mkdir()
        (nxt / "assets" / "n.png").write_text("png")
        (nxt / "index.mdx").write_text(
            'See [workers](/next/using-iii/workers) and [old](/0-11-0/gone). '
            '<Card href="/next/using-iii/workers" /> <img src="/next/assets/n.png" />'
        )
        _write_docs(docs, [
            {"version": "0.16.x", "tag": "Latest", "default": True,
             "tabs": [{"tab": "D", "groups": [{"group": "G",
                "pages": ["index", "using-iii/workers"]}]}]},
            {"version": "0.17.x", "tag": "Next",
             "tabs": [{"tab": "D", "groups": [{"group": "G",
                "pages": ["next/index", "next/using-iii/workers"]}]}]},
        ])
        rotate(docs, "0.17.0")

        # Archived old Latest: root-absolute link + asset src re-pointed into 0-16-0/.
        archived_index = (docs / "0-16-0" / "index.mdx").read_text()
        assert "(/0-16-0/using-iii/workers)" in archived_index
        assert 'src="/0-16-0/assets/pic.png"' in archived_index
        # Promoted Latest (root): next/-prefix stripped to root-absolute (links,
        # href and src); legacy cross-version ref to a missing page left untouched.
        root_index = (docs / "index.mdx").read_text()
        assert "(/using-iii/workers)" in root_index
        assert 'href="/using-iii/workers"' in root_index  # Card href stripped too
        assert 'src="/assets/n.png"' in root_index        # img src stripped too
        assert "/next/" not in root_index
        assert "(/0-11-0/gone)" in root_index
        # Next folder is reused untouched — its links still point at next/.
        assert "(/next/using-iii/workers)" in (docs / "next" / "index.mdx").read_text()


class TestSyncPatch:
    def test_refreshes_latest_without_rotation(self, tmp_path):
        docs = tmp_path / "docs"
        docs.mkdir()
        (docs / "index.mdx").write_text("# old latest")
        nxt = docs / "next"
        (nxt / "using-iii").mkdir(parents=True)
        (nxt / "using-iii" / "workers.mdx").write_text("# w")
        (nxt / "index.mdx").write_text(
            '[w](/next/using-iii/workers) <Card href="/next/using-iii" />'
        )
        _write_docs(docs, [
            {"version": "0.17.x", "tag": "Latest", "default": True,
             "tabs": [{"tab": "D", "groups": [{"group": "G", "pages": ["index"]}]}]},
            {"version": "0.18.x", "tag": "Next",
             "tabs": [{"tab": "D", "groups": [{"group": "G",
                "pages": ["next/index", "next/using-iii/workers"]}]}]},
        ])
        before = (docs / "docs.json").read_text()
        sync_patch(docs)

        # Root refreshed from next/, links re-pointed to root.
        root_index = (docs / "index.mdx").read_text()
        assert "# old latest" not in root_index
        assert "(/using-iii/workers)" in root_index
        assert 'href="/using-iii"' in root_index
        assert "/next/" not in root_index
        # Next folder untouched.
        assert "(/next/using-iii/workers)" in (docs / "next" / "index.mdx").read_text()
        # No archive folder, and docs.json version blocks unchanged.
        assert not (docs / "0-17-0").exists()
        assert (docs / "docs.json").read_text() == before
