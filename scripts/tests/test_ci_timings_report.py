import contextlib
import io
import unittest
from unittest import mock

from script_loader import ROOT, load

report = load("ci_timings_report", ROOT / "scripts" / "ci-timings-report.py")


class ParseDiskHeadroomTests(unittest.TestCase):
    """The log-line parsing contract with `gh run view --log` output."""

    def test_telemetry_line_parses_to_lane_and_mb(self) -> None:
        lines = [
            "ubuntu-lane\tReport disk headroom\tdisk-headroom: 18345 MB free of 73728 MB\n",
        ]
        self.assertEqual(
            report.parse_disk_headroom(lines), {"ubuntu-lane": "18345"}
        )

    def test_non_telemetry_and_malformed_lines_are_skipped(self) -> None:
        lines = [
            "ubuntu-lane\tReport disk headroom\tFilesystem  Size  Used\n",
            "no tabs in this line\n",
            "ubuntu-lane\tReport disk headroom\tdisk-headroom: 9500 MB free of 73728 MB\n",
        ]
        self.assertEqual(report.parse_disk_headroom(lines), {"ubuntu-lane": "9500"})

    def test_empty_log_yields_empty_map(self) -> None:
        # A red run whose lanes died before the telemetry step must degrade
        # to {} so the report still renders job timings.
        self.assertEqual(report.parse_disk_headroom([]), {})


class FetchLogTests(unittest.TestCase):
    """fetch_log is loud-tolerant: a failed download warns on stderr."""

    def test_failed_download_warns_and_returns_empty(self) -> None:
        proc = mock.Mock(returncode=1, stdout="", stderr="boom\n")
        with mock.patch.object(report, "_gh", return_value=proc) as gh:
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                self.assertEqual(report.fetch_log("123"), "")
        self.assertIn(
            "warning: gh run view --log failed: boom", stderr.getvalue()
        )
        gh.assert_called_once_with("run", "view", "123", "--log", check=False)

    def test_successful_download_returns_stdout(self) -> None:
        proc = mock.Mock(returncode=0, stdout="lane\tstep\tdisk-headroom: 1 MB\n")
        with mock.patch.object(report, "_gh", return_value=proc):
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                self.assertEqual(
                    report.fetch_log("123"),
                    "lane\tstep\tdisk-headroom: 1 MB\n",
                )
        self.assertEqual(stderr.getvalue(), "")


class RenderReportTests(unittest.TestCase):
    """render_report takes the parsed run payload (fetch_run parses)."""

    def test_renders_rows_from_parsed_payload(self) -> None:
        data = {
            "displayTitle": "CI",
            "createdAt": "2026-10-15T00:00:00Z",
            "jobs": [
                {
                    "name": "Test (engine)",
                    "conclusion": "success",
                    "startedAt": "2026-10-15T00:00:00Z",
                    "completedAt": "2026-10-15T00:01:30Z",
                }
            ],
        }
        out = report.render_report("1", data, {"Test (engine)": "9000"})
        self.assertIn("| Test (engine) | success | 1m30s | 9000 |", out)
        self.assertIn("<!-- ci-timings-report: run 1 (CI, 2026-10-15T00:00:00Z) -->", out)

    def test_job_without_timestamps_is_skipped(self) -> None:
        data = {
            "displayTitle": "CI",
            "createdAt": "2026-10-15T00:00:00Z",
            "jobs": [{"name": "queued", "conclusion": None}],
        }
        out = report.render_report("1", data, {})
        self.assertNotIn("| queued", out)


if __name__ == "__main__":
    unittest.main()
