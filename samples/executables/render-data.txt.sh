#!/bin/sh

# Ensure that we get the same output whether served over HTTP or not.
echo "$OPERATOR_RENDER_DATA" \
    | sed -E 's/"socket-address":"[^"]*"/"socket-address":null/' \
    | sed -E 's/"socket-address":null/"socket-address":$SOCKET_ADDRESS/'
