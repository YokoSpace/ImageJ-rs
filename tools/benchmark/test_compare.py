import json
import os
import platform
import tempfile
import unittest
from pathlib import Path

import compare


def result(**overrides):
    base = {"case": "invert", "type": 8, "sigma": 2.0, "input": "noise", "seed": 1,
            "width": 8, "height": 8, "slices": 2, "iterations": 3, "elapsed_ns": 123,
            "fingerprint": "abcd",
            "threads": {"requested": None, "resolved": 14, "policy": "prefs"},
            "machine_threads": 14}
    base.update(overrides)
    return base


def settings(**overrides):
    base = {"cases": ["invert"], "width": 64, "height": 48, "slices": 2, "iterations": 3,
            "warmup": 1, "runs": 1, "profile": "release", "type_bits": 8, "sigma": 2.0,
            "input_name": "noise", "seed": compare.DEFAULT_SEED, "java_threads": None,
            "rust_threads": None}
    base.update(overrides)
    return compare.build_settings(**base)


class BenchmarkProtocolTests(unittest.TestCase):
    def test_result_parser_requires_the_single_protocol_line(self):
        output = ('status text\nBENCH_RESULT={"case":"invert","type":8,"sigma":2.0,'
                  '"input":"noise","seed":1,"width":8,"height":8,"slices":2,"iterations":3,'
                  '"elapsed_ns":123,"fingerprint":"abcd",'
                  '"threads":{"requested":null,"resolved":14,"policy":"prefs"},'
                  '"machine_threads":14}\n')
        self.assertEqual(compare.parse_result(output)["elapsed_ns"], 123)
        with self.assertRaises(ValueError):
            compare.parse_result("status text only")

    def test_result_parser_rejects_incomplete_or_ambiguous_results(self):
        complete = result()
        for key in ("type", "sigma", "input", "seed", "threads", "machine_threads"):
            broken = dict(complete)
            del broken[key]
            with self.assertRaisesRegex(ValueError, key):
                compare.parse_result(f"BENCH_RESULT={json.dumps(broken)}")
        with self.assertRaisesRegex(ValueError, "elapsed_ns"):
            compare.parse_result(
                f"BENCH_RESULT={json.dumps(result(elapsed_ns=0))}")
        partial_threads = json.dumps(result(threads={"resolved": 4}))
        with self.assertRaisesRegex(ValueError, "threads"):
            compare.parse_result(f"BENCH_RESULT={partial_threads}")
        zero_threads = json.dumps(
            result(threads={"requested": None, "resolved": 0, "policy": "prefs"}))
        with self.assertRaisesRegex(ValueError, "threads.resolved"):
            compare.parse_result(f"BENCH_RESULT={zero_threads}")

    def test_peak_rss_parsers_use_platform_units(self):
        mac = "  104857600  maximum resident set size\n"
        linux = "Maximum resident set size (kbytes): 102400\n"
        self.assertEqual(compare.parse_peak_rss_mib(mac, "darwin"), 100.0)
        self.assertEqual(compare.parse_peak_rss_mib(linux, "linux"), 100.0)

    def test_mismatched_outputs_are_rejected(self):
        java = result()
        for key, value in (("fingerprint", "bbb"), ("type", 16), ("sigma", 2.5),
                           ("input", "impulse"), ("seed", 2), ("width", 9)):
            with self.assertRaisesRegex(ValueError, key):
                compare.check_equivalent(java, result(**{key: value}))

    def test_thread_counts_are_recorded_but_not_compared(self):
        java = result(threads={"requested": 1, "resolved": 1, "policy": "prefs"})
        rust = result(threads={"requested": None, "resolved": None, "policy": "sequential"})
        compare.check_equivalent(java, rust)

    def test_summary_uses_median_per_operation(self):
        samples = [
            {"case": "invert", "engine": "java", "elapsed_ns": 20_000_000,
             "iterations": 2, "peak_rss_mib": 100.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": 14, "policy": "prefs"}},
            {"case": "invert", "engine": "java", "elapsed_ns": 60_000_000,
             "iterations": 2, "peak_rss_mib": 120.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": 14, "policy": "prefs"}},
            {"case": "invert", "engine": "rust", "elapsed_ns": 10_000_000,
             "iterations": 2, "peak_rss_mib": 20.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": None, "policy": "sequential"}},
            {"case": "invert", "engine": "rust", "elapsed_ns": 30_000_000,
             "iterations": 2, "peak_rss_mib": 30.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": None, "policy": "sequential"}},
        ]
        row = compare.summarize(samples, ["invert"])[0]
        self.assertEqual(row["java_ms_per_op"], 20.0)
        self.assertEqual(row["rust_ms_per_op"], 10.0)
        self.assertEqual(row["java_peak_rss_mib"], 110.0)
        self.assertEqual(row["speed_ratio_java_over_rust"], 2.0)
        self.assertEqual(row["java_threads_resolved"], 14)
        self.assertIsNone(row["rust_threads_resolved"])
        self.assertEqual(row["rust_thread_policy"], "sequential")
        self.assertEqual(row["machine_threads"], 14)

    def test_summary_rejects_thread_counts_that_change_between_runs(self):
        samples = [
            {"case": "invert", "engine": "java", "elapsed_ns": 20_000_000,
             "iterations": 2, "peak_rss_mib": 100.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": 14, "policy": "prefs"}},
            {"case": "invert", "engine": "java", "elapsed_ns": 20_000_000,
             "iterations": 2, "peak_rss_mib": 100.0, "machine_threads": 14,
             "threads": {"requested": 1, "resolved": 1, "policy": "prefs"}},
            {"case": "invert", "engine": "rust", "elapsed_ns": 10_000_000,
             "iterations": 2, "peak_rss_mib": 20.0, "machine_threads": 14,
             "threads": {"requested": None, "resolved": None, "policy": "sequential"}},
        ]
        with self.assertRaisesRegex(ValueError, "Java threads"):
            compare.summarize(samples, ["invert"])


class SettingsTests(unittest.TestCase):
    def test_java_numeric_domain_is_checked_before_launch(self):
        for name in ("iterations", "warmup", "java_threads", "rust_threads"):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "2147483647"):
                settings(**{name: 2 ** 31})

    def test_defaults_match_the_documented_baseline(self):
        parsed = settings()
        self.assertEqual(parsed["type"], 8)
        self.assertEqual(parsed["sigma"], 2.0)
        self.assertEqual(parsed["input"], "noise")
        self.assertEqual(parsed["seed"], compare.DEFAULT_SEED)
        self.assertIsNone(parsed["java_threads"])
        self.assertIsNone(parsed["rust_threads"])

    def test_invalid_settings_are_rejected(self):
        for overrides, message in (
            ({"cases": []}, "at least one case"),
            ({"cases": ["blur"]}, "unknown case"),
            ({"width": 0}, "width must be positive"),
            ({"slices": 0}, "slices must be positive"),
            ({"iterations": 0}, "iterations must be positive"),
            ({"runs": 0}, "runs must be positive"),
            ({"warmup": -1}, "warmup must be non-negative"),
            ({"profile": "fast"}, "unknown profile"),
            ({"type_bits": 24}, "unknown type"),
            ({"sigma": 0.0}, "sigma must be finite"),
            ({"sigma": -1.0}, "sigma must be finite"),
            ({"sigma": float("nan")}, "sigma must be finite"),
            ({"sigma": 1e9}, "sigma must be finite"),
            ({"input_name": "random"}, "unknown input"),
            ({"seed": -1}, "seed must be an unsigned"),
            ({"seed": 2 ** 64}, "seed must be an unsigned"),
            ({"java_threads": 0}, "java-threads must be positive"),
            ({"rust_threads": 0}, "rust-threads must be positive"),
            ({"width": 100_000, "height": 1_000, "slices": 2}, "exceeds"),
        ):
            with self.assertRaisesRegex(ValueError, message):
                settings(**overrides)

    def test_worker_options_are_ordered_and_engine_specific(self):
        parsed = settings(type_bits=32, sigma=1.5, input_name="impulse", seed=7,
                          java_threads=4, rust_threads=2)
        self.assertEqual(compare.worker_options(parsed, parsed["java_threads"]),
                         ["type=32", "sigma=1.5", "input=impulse", "seed=7", "threads=4"])
        self.assertEqual(compare.worker_options(parsed, parsed["rust_threads"]),
                         ["type=32", "sigma=1.5", "input=impulse", "seed=7", "threads=2"])
        self.assertEqual(compare.worker_options(parsed, None),
                         ["type=32", "sigma=1.5", "input=impulse", "seed=7"])

    def test_thread_and_seed_parsers(self):
        self.assertIsNone(compare.parse_threads("default"))
        self.assertEqual(compare.parse_threads("4"), 4)
        for value in ("0", "-1", "many"):
            with self.assertRaises(Exception):
                compare.parse_threads(value)
        self.assertEqual(compare.parse_seed("0x10"), 16)
        self.assertEqual(compare.parse_seed("16"), 16)
        self.assertEqual(compare.parse_seed("01"), 1)
        for value in ("-1", "0x1p3", "2**64"):
            with self.assertRaises(Exception):
                compare.parse_seed(value)


@unittest.skipUnless(os.environ.get("IJ_BENCH_SMOKE") == "1",
                     "set IJ_BENCH_SMOKE=1 to run the Java/Rust smoke test")
class JavaRustSmokeTests(unittest.TestCase):
    """Runs both engines on a tiny image and requires identical fingerprints.

    Gated because it builds the oracle and the release CLI; it makes no timing
    assertions, so it is safe to run anywhere but too slow for every CI pass.
    """

    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="ij-bench-smoke-")
        cls.java_command, cls.rust_command = compare.build_workers(
            "release", Path(cls.temporary.name))
        cls.system = platform.system().lower()

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_every_case_and_type_matches_the_oracle(self):
        for type_bits in compare.TYPES:
            for case in compare.CASES:
                for input_name in compare.INPUTS:
                    parsed = settings(cases=[case], type_bits=type_bits, input_name=input_name,
                                      width=32, height=24, slices=2, iterations=1, warmup=0)
                    with self.subTest(type=type_bits, case=case, input=input_name):
                        pair = compare.invoke_pair(
                            self.java_command, self.rust_command, case,
                            ["32", "24", "2", "1", "0"], parsed, self.system)
                        self.assertEqual(pair["java"]["fingerprint"],
                                         pair["rust"]["fingerprint"])

    def test_java_thread_count_is_applied_and_reported(self):
        parsed = settings(cases=["gaussian"], java_threads=1, width=32, height=24,
                          slices=2, iterations=1, warmup=0)
        pair = compare.invoke_pair(self.java_command, self.rust_command, "gaussian",
                                   ["32", "24", "2", "1", "0"], parsed, self.system)
        self.assertEqual(pair["java"]["threads"]["requested"], 1)
        self.assertEqual(pair["java"]["threads"]["resolved"], 1)
        self.assertEqual(pair["java"]["threads"]["policy"], "prefs")
        self.assertEqual(pair["rust"]["threads"]["policy"], "session-default")
        self.assertGreaterEqual(pair["rust"]["threads"]["resolved"], 1)

    def test_java_default_budget_loads_preferences_before_resolution(self):
        with tempfile.TemporaryDirectory(prefix="ij-bench-prefs-") as directory:
            (Path(directory) / "IJ_Prefs.txt").write_text("threads=1\n")
            command = [self.java_command[0], f"-Duser.dir={directory}",
                       *self.java_command[1:], "gaussian", "8", "8", "1", "1", "0"]
            sample = compare.invoke(command, self.system)
            self.assertEqual(sample["threads"]["resolved"], 1)

    def test_rust_worker_budget_is_applied_and_does_not_change_pixels(self):
        default = settings(cases=["gaussian"], width=32, height=24, slices=2,
                           iterations=1, warmup=0)
        single = settings(cases=["gaussian"], width=32, height=24, slices=2,
                          iterations=1, warmup=0, rust_threads=1)
        worker_args = ["32", "24", "2", "1", "0"]
        baseline = compare.invoke_pair(self.java_command, self.rust_command, "gaussian",
                                       worker_args, default, self.system)
        limited = compare.invoke_pair(self.java_command, self.rust_command, "gaussian",
                                      worker_args, single, self.system)
        self.assertEqual(limited["rust"]["threads"]["requested"], 1)
        self.assertEqual(limited["rust"]["threads"]["resolved"], 1)
        self.assertEqual(limited["rust"]["threads"]["policy"], "explicit-workers")
        self.assertEqual(baseline["rust"]["fingerprint"], limited["rust"]["fingerprint"])

    def test_sequential_cases_report_no_worker_budget(self):
        parsed = settings(cases=["invert"], width=32, height=24, slices=2,
                          iterations=1, warmup=0)
        pair = compare.invoke_pair(self.java_command, self.rust_command, "invert",
                                   ["32", "24", "2", "1", "0"], parsed, self.system)
        self.assertIsNone(pair["rust"]["threads"]["resolved"])
        self.assertEqual(pair["rust"]["threads"]["policy"], "sequential")
        self.assertIsNone(pair["java"]["threads"]["resolved"])
        self.assertEqual(pair["java"]["threads"]["policy"], "sequential")


if __name__ == "__main__":
    unittest.main()
