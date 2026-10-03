// Ressources liées au pipeline, communes à tous les shaders : UBO (set 0) et
// push constants. ⚠ Doit rester synchronisé avec `UniformBufferObject`
// (uniform_buffer.rs) : même ordre de champs.
#pragma once

// set 0, binding 0 : données « caméra », mises à jour une fois par frame.
struct UniformBuffer {
    float4x4 view;
    float4x4 proj;
    float4 cameraPosition;
    float4x4 lightViewProj; // view*proj de la lumière (projection orthographique)
    float4 lightDirection;  // xyz = direction de propagation des rayons
    float4 lightColor;      // rgb = couleur, w = intensité
    float4 fog;             // rgb = couleur de l'horizon, w = σ₀
    float4 fogParams;       // x = H, y = anisotropie du halo, z = intensité du halo
    float4 shadowParams;    // x = épaisseur de la boîte d'ombre, y = texel (unités monde)
    float4 sunDisk;         // x = rayon angulaire (rad), y = fraction du rayon où commence le fondu, z = intensité
    float4 skyZenith;       // rgb = couleur du zénith, w = exposant du dégradé horizon → zénith
    float4 ambient;         // rgb = lumière ambiante du ciel, w = part renvoyée par le sol
    float4x4 skyRotation;   // rotation monde → ciel fixe (partie 3x3)
    float4 stars;           // x = luminosité × visibilité, y = temps réel (s), z = densité, w = scintillation
};
[[vk::binding(0, 0)]]
ConstantBuffer<UniformBuffer> ubo;

// Push constant : matrice « model » + matrice normale (inverse-transposée de la
// 3x3 du model, calculée côté CPU), poussées par objet / par chunk de terrain.
struct PushConstants {
    float4x4 model;
    float4x4 normalMatrix;
};
[[vk::push_constant]]
ConstantBuffer<PushConstants> pc;
