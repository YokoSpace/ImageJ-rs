import unittest

import compare


class BenchmarkProtocolTests(unittest.TestCase):
    def test_result_parser_requires_the_single_protocol_line(self):
        output = ('status text\nBENCH_RESULT={"case":"invert","width":8,"height":8,'
                  '"slices":2,"iterations":3,"elapsed_ns":123,"fingerprint":"abcd"}\n')
        self.assertEqual(compare.parse_result(output)["elapsed_ns"], 123)
        with self.assertRaises(ValueError):
            compare.parse_result("status text only")

    def test_peak_rss_parsers_use_platform_units(self):
        mac = "  104857600  maximum resident set size\n"
        linux = "Maximum resident set size (kbytes): 102400\n"
        self.assertEqual(compare.parse_peak_rss_mib(mac, "darwin"), 100.0)
        self.assertEqual(compare.parse_peak_rss_mib(linux, "linux"), 100.0)

    def test_mismatched_outputs_are_rejected(self):
        java = {"case": "invert", "width": 8, "height": 8, "slices": 2,
                "iterations": 3, "fingerprint": "aaa", "elapsed_ns": 100}
        rust = dict(java, fingerprint="bbb")
        with self.assertRaisesRegex(ValueError, "fingerprint"):
            compare.check_equivalent(java, rust)

    def test_summary_uses_median_per_operation(self):
        samples = [
            {"case": "invert", "engine": "java", "elapsed_ns": 20_000_000,
             "iterations": 2, "peak_rss_mib": 100.0},
            {"case": "invert", "engine": "java", "elapsed_ns": 60_000_000,
             "iterations": 2, "peak_rss_mib": 120.0},
            {"case": "invert", "engine": "rust", "elapsed_ns": 10_000_000,
             "iterations": 2, "peak_rss_mib": 20.0},
            {"case": "invert", "engine": "rust", "elapsed_ns": 30_000_000,
             "iterations": 2, "peak_rss_mib": 30.0},
        ]
        row = compare.summarize(samples, ["invert"])[0]
        self.assertEqual(row["java_ms_per_op"], 20.0)
        self.assertEqual(row["rust_ms_per_op"], 10.0)
        self.assertEqual(row["java_peak_rss_mib"], 110.0)
        self.assertEqual(row["speed_ratio_java_over_rust"], 2.0)


if __name__ == "__main__":
    unittest.main()
