// Ciel : un triangle plein écran au plan far, dessiné après la géométrie opaque.
// La pipeline (test LESS_OR_EQUAL, sans écriture) ne le laisse passer que là où
// rien n'a été dessiné. Sous la surface de la mer, il peint l'eau profonde.
#include "lib/atmosphere.slang"
#include "lib/stars.slang"
#include "lib/water.slang"

struct SkyVSOutput {
  float4 pos : SV_Position;
  // Direction de vue en espace monde, NON normalisée : linéaire en NDC, donc
  // l'interpolation du rasterizer est exacte ; on normalise par fragment.
  float3 worldRay;
}

[shader("vertex")]
SkyVSOutput skyVertMain(uint vertexId : SV_VertexID) {
  // 0 → (-1,-1), 1 → (3,-1), 2 → (-1,3) : un triangle qui couvre [-1,1]².
  float2 ndc = float2(vertexId == 1 ? 3.0 : -1.0, vertexId == 2 ? 3.0 : -1.0);

  SkyVSOutput output;
  // z = 0 → profondeur 0.0 : plan far en reversed-Z, valeur du clear.
  output.pos = float4(ndc, 0.0, 1.0);

  // NDC → espace vue : la projection ne fait que multiplier x et y par
  // proj[0][0] et proj[1][1] (le flip Y Vulkan est dans le signe de ce dernier).
  float3 viewRay = float3(ndc.x / ubo.proj[0][0], ndc.y / ubo.proj[1][1], -1.0);
  // Espace vue → monde : inverse de la rotation de `view` = sa transposée.
  output.worldRay = mul(transpose((float3x3)ubo.view), viewRay);
  return output;
}

[shader("fragment")]
float4 skyFragMain(SkyVSOutput vertIn) : SV_Target {
  // Sous l'eau, l'infini est de l'eau : ni ciel, ni étoiles, ni soleil.
  if (cameraUnderSea())
    return float4(deepWaterColor(), 1.0);

  float3 V = normalize(vertIn.worldRay);
  float3 L = normalize(-ubo.lightDirection.xyz);
  float3 sunCol = ubo.lightColor.rgb * ubo.lightColor.w;

  // Regard qui plonge dans la mer sans fond dessiné (au-delà du terrain chargé) : fond
  // à l'infini, donc limite de `underwater`, brumée jusqu'à la surface comme le terrain.
  // Sans cela, l'eau (semi-transparente) laisserait voir le ciel à travers elle.
  float3 camPos = ubo.cameraPosition.xyz;
  if (V.z < 0.0 && camPos.z > seaLevel()) {
    float3 surface = camPos + V * ((camPos.z - seaLevel()) / -V.z);
    if (seaCovers(surface.xy))
      return float4(applyFog(deepWaterColor(), surface, L, sunCol), 1.0);
  }

  float3 color = airColor(V, L, sunCol) + stars(V) + sunDisk(V, L, sunCol);
  return float4(color, 1.0);
}
