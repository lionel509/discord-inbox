#!/bin/sh
# Canned stand-in for the receipt verdict command (#31 demo). Reads the prompt on stdin and ignores it.
# Every line is a canned stub answer, not a model's reading of the photo.
cat > /dev/null
echo "note: canned stub model answer"
echo "receipt: yes"
echo "merchant: 華聯"
echo "total: 9.70"
echo "category: groceries"
