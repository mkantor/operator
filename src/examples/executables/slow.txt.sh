#!/bin/sh

printf "\xEF\xBB\xBF" # UTF-8 BOM
echo 🔴 Ready…
sleep 1
echo 🟡 Set…
sleep 1
echo 🟢 Go!