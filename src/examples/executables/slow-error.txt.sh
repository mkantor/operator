#!/bin/sh

echo "Self-destructing in two seconds..."
sleep 2
echo "Boom!" 1>&2 
exit 1
