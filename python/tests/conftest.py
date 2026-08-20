import sys

# Files containing t-string syntax cannot even parse before Python 3.14.
collect_ignore = [] if sys.version_info >= (3, 14) else ["test_tstring.py"]
