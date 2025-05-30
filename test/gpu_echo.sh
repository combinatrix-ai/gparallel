#!/usr/bin/env bash

# Show the current time, CUDA_VISIBLE_DEVICES, and PID
echo "[$(date '+%H:%M:%S')] CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-'(not set)'}  PID=$$"
# Loop 10 seconds while sleeping 1 second each iteration
for i in {1..5}; do
  echo "[$(date '+%H:%M:%S')] iteration $i CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-'(not set)'}  PID=$$"
  sleep 1
done

echo "[$(date '+%H:%M:%S')] done CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-'(not set)'}  PID=$$"
