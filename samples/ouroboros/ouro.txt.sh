#!/bin/sh

render_data() {
  echo "$OPERATOR_RENDER_DATA" \
    | ./.JSON.sh \
    | grep -E "\[$1\]" \
    | cut -f2 \
    | sed -e 's/^"//' -e 's/"$//'
}

request_route=$(render_data '"request","route"')
socket_address=$(render_data '"server-info","socket-address"')
operator=$(render_data '"server-info","operator-path"')

echo "ouro: ${request_route}"
if [ "$request_route" != "/boros" ]
then
  if [ "$socket_address" = "null" ]
  then
    "$operator" get --content-directory=. --route=/boros
  else
    curl -sS "$socket_address/boros"
  fi
fi
