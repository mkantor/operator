#!/bin/sh

render_data_without_socket_address=$(
    echo "$OPERATOR_RENDER_DATA" \
        | sed -E 's/"socket-address":"[^"]*"/"socket-address":null/' \
        | sed -E 's/"socket-address":null/"socket-address":"$SOCKET_ADDRESS"/'
)

# Ensure that snapshot tests produce the same output for HTTP and get command.
if [ "${render_data_without_socket_address#*'"is-operator-snapshot-test":"true"'}" != "$render_data_without_socket_address" ]
then
    echo "$render_data_without_socket_address" \
        | sed -E 's/"request-headers":\{[^}]*\}/"request-headers":\{\}/'
else
    echo "$render_data_without_socket_address"
fi