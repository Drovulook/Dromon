// Ressources liées au pipeline, communes à tous les shaders : UBO (set 0) et
// push constants. ⚠ Doit rester synchronisé avec `UniformBufferObject`
// (uniform_buffer.rs) : même ordre de champs.
#pragma once

// Nombre de vagues de la houle. ⚠ Synchronisé avec `WAVE_COUNT` (world/water/waves.rs).
static const int WAVE_COUNT = 8;

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
    float4 stars;           // x = luminosité × visibilité, y inutilisé, z = densité, w = scintillation
    float4 worldMaps;       // passage monde → UV des cartes du monde : uv = (xy − x) · y
    float4 water;           // x = sea_level, y = portée horizontale, z = marge terre, w = 1 si mer
    float4 waterAbsorption; // xyz = k par canal (par unité), w = 1 si caméra sous la mer
    float4 waterDeepColor;  // xyz = couleur de l'eau profonde, w = variance des pentes (reflet)
    float4 time;            // x = temps réel (s)
    float4 waves[2 * WAVE_COUNT]; // houle : 2 par vague, (direction, k, A) puis (ω, phase, –, –)
    float4 waveParams;      // x = coast_fade
    // Écume : (albedo, zone_depth, patch_size, bubble_size),
    // (bubble_speed, drift, surge_period, surge_spread), (calm_zone, calm_density, –, –)
    float4 foam[3];
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
