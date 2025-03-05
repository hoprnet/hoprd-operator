#!/bin/bash
# Function to fetch metrics from Hopr API
fetch_metrics() {
  curl -s --max-time 10 -H 'accept: application/json' -H "X-Auth-Token: ${HOPRD_API_TOKEN}" "http://localhost:3001/api/v3/node/metrics"
  if [ $? -ne 0 ]; then
    echo "Error: Failed to fetch Hopr metrics"
  fi
}
# Output the headers
echo "Content-Type: text/plain; charset=utf-8"
echo ""
# Output the metrics
echo "$(fetch_metrics)"