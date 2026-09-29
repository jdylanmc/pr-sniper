"""Credential-free integration check, restricted to disposable hosted macOS CI."""

import os
from pathlib import Path
import platform
import secrets
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import sign_release


@unittest.skipUnless(
    platform.system() == "Darwin" and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted",
    "Keychain mutation is tested only on disposable hosted macOS runners, never a developer Mac.",
)
class NativeKeychainTests(unittest.TestCase):
    def test_owned_empty_keychain_search_list_is_added_and_restored(self):
        before = sign_release.keychain_search_list()
        with tempfile.TemporaryDirectory(prefix="pr-sniper-keychain-test-", dir=os.environ["RUNNER_TEMP"]) as root:
            keychain = str(Path(root) / "empty.keychain-db")
            created = False
            try:
                sign_release.run(["security", "create-keychain", "-p", secrets.token_urlsafe(32), keychain])
                created = True
                sign_release.set_keychain_search_list([keychain] + before)
                self.assertEqual(sign_release.keychain_search_list(), [keychain] + before)
            finally:
                try:
                    sign_release.set_keychain_search_list(before)
                finally:
                    if created:
                        sign_release.run(["security", "delete-keychain", keychain])
            self.assertEqual(sign_release.keychain_search_list(), before)


if __name__ == "__main__":
    unittest.main()
