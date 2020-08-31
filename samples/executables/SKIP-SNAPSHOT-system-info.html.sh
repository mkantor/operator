#!/bin/sh

echo "
    <dl>
        <dt>date:</dt>
        <dd><pre>$(date)</pre></dd>

        <dt>env:</dt>
        <dd><pre>$(env)</pre></dd>

        <dt>logname:</dt>
        <dd><pre>$(logname)</pre></dd>

        <dt>ps:</dt>
        <dd><pre>$(ps)</pre></dd>

        <dt>pwd:</dt>
        <dd><pre>$(pwd)</pre></dd>

        <dt>uname:</dt>
        <dd><pre>$(uname -a)</pre></dd>

        <dt>who:</dt>
        <dd><pre>$(who)</pre></dd>
    </dl>
"
