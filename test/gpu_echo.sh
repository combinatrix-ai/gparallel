#!/usr/bin/env bash
# -------------------------------------
# 1) 割り当てられた GPU ID を表示
# 2) 1 秒スリープして終了
# -------------------------------------

echo "[$(date '+%H:%M:%S')] CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES:-'(not set)'}  PID=$$"
sleep 1
echo "[$(date '+%H:%M:%S')] done  PID=$$"
