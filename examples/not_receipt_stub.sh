#!/bin/sh
# Canned stand-in for the receipt verdict command (#67 demo). Reads the prompt on stdin and ignores it.
# It always answers "not a receipt", so the warm-reply demo has no receipt line to show.
cat > /dev/null
echo "note: canned stub model answer"
echo "receipt: no"
