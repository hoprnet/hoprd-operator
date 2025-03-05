#!/bin/bash

sed -i 's/${HOPRD_API_TOKEN}/'$HOPRD_API_TOKEN'/' /var/www/cgi-bin/metrics.sh
service lighttpd start
sleep infinity
