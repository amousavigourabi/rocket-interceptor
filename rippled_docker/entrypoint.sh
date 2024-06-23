#!/bin/bash

echo "Args:"
echo "    >> [$@]"
echo ""
echo "Env Args:"
echo "    >> [$ENV_ARGS]"
echo ""
echo "Env:"
printenv
echo ""

rippledconfig=`/bin/cat /config/rippled.cfg 2>/dev/null | wc -l`
validatorstxt=`/bin/cat /config/validators.txt 2>/dev/null | wc -l`
ledgerjson=`/bin/cat /config/ledger.json 2>/dev/null | wc -l`

mkdir -p /config

if [[ "$rippledconfig" -gt "0" ]]; then
    echo "Existing rippled config at host /config/, using."
    /bin/cat /config/rippled.cfg > /etc/opt/ripple/rippled.cfg
fi

if [[ "$validatorstxt" -gt "0" ]]; then
    echo "Existing validator config at host /config/, using."
    /bin/cat /config/validators.txt > /etc/opt/ripple/validators.txt
fi

if [[ "$ledgerjson" -gt "0" ]]; then
    echo "Existing ledger json at host /config/, using."
    /bin/cat /config/ledger.json > /etc/opt/ripple/ledger.json
fi

# Start rippled, Passthrough other arguments
exec /opt/ripple/bin/rippled --conf /etc/opt/ripple/rippled.cfg $@$ENV_ARGS
