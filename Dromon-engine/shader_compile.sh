#!/bin/bash
set -e

slangc dev_res/shaders/shader.slang -entry vertMain       -stage vertex   -o res/shaders/vert.spv
slangc dev_res/shaders/shader.slang -entry fragMain       -stage fragment -o res/shaders/frag.spv
slangc dev_res/shaders/shader.slang -entry shadowVertMain -stage vertex   -o res/shaders/shadow_vert.spv

echo "Shaders compilés."
