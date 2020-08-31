#!/bin/sh

# Usually exits with no output, but prints "win" if you get lucky.
random_number=$(od -vAn -N1 -tu1 </dev/urandom)
if [ "$random_number" -lt 32 ]
then
  echo "win"
fi
