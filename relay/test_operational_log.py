import logging
import unittest
from unittest.mock import Mock, patch

from relay import operational_log as diagnostics


class OperationalLogTests(unittest.TestCase):
    def setUp(self):
        diagnostics._recent.clear()
        self.logger = Mock(spec=logging.Logger)
        self.logger.name = "test.operational"

    def tearDown(self):
        diagnostics._recent.clear()

    def test_repeated_failures_are_bounded_and_report_suppressed_count(self):
        with patch.object(diagnostics.time, "monotonic", return_value=0):
            for _ in range(10_000):
                diagnostics._log_operational_failure(self.logger, "fetch", ValueError("token=private"))
        self.assertEqual(self.logger.log.call_count, 1)
        with patch.object(diagnostics.time, "monotonic", return_value=300):
            diagnostics._log_operational_failure(self.logger, "fetch", ValueError("token=private"))
        self.assertEqual(self.logger.log.call_count, 2)
        self.assertEqual(self.logger.log.call_args.args[-1], 9999)
        self.assertNotIn("private", str(self.logger.log.call_args_list))

    def test_different_operations_are_independent_and_state_is_bounded(self):
        with patch.object(diagnostics.time, "monotonic", return_value=0):
            for index in range(200):
                diagnostics._log_operational_failure(self.logger, str(index), status=503)
        self.assertEqual(self.logger.log.call_count, 200)
        self.assertEqual(len(diagnostics._recent), 128)
        self.assertEqual(self.logger.log.call_args.args[-2], 503)


if __name__ == "__main__":
    unittest.main()
