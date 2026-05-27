#!/bin/bash
set -e

glslc dev_res/shaders/shader.frag -o res/shaders/frag.spv
glslc dev_res/shaders/shader.vert -o res/shaders/vert.spv

echo "Shaders compilés."
