// Table des matériaux (set 4) : SSBO indexé par ID (0 = air, puis materials.ron dans
// l'ordre, puis le matériau de debug). ⚠ Disposition synchronisée avec `GpuMaterial`
// (material_buffer.rs).
#pragma once

struct Material {
  float4 albedo;    // rgb = albédo linéaire (humide si teinté)
  float4 dryAlbedo; // rgb = albédo en zone sèche (= albedo si non teinté)
  float4 humidity;  // x, y = précipitations du fondu sec → humide
};

[[vk::binding(0, 4)]]
StructuredBuffer<Material> materials;

// Albédo du matériau `id` sous `precipitation` (normalisées, cf. `precipitation()`).
// Même fondu pour tous : un matériau non teinté a `dryAlbedo == albedo`.
float3 materialAlbedo(uint id, float precipitation) {
  Material m = materials[id];
  float wet = smoothstep(m.humidity.x, m.humidity.y, precipitation);
  return lerp(m.dryAlbedo.rgb, m.albedo.rgb, wet);
}
