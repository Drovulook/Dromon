// Surface de la mer (CDLOD) : une grille unique redessinée par nœud, placée par
// push constants. Étape 1 : couleur et alpha constants, pas de vagues.

#include "lib/atmosphere.slang"

// ⚠ Même disposition que `WaterPushConstants` (world/water.rs).
struct WaterPushConstants {
  float2 origin;   // coin (x min, y min) du nœud, unités monde
  float cellSize;  // côté d'une cellule de la grille
  float seaLevel;
};
[[vk::push_constant]]
ConstantBuffer<WaterPushConstants> waterPc;

static const float3 WATER_COLOR = float3(0.05, 0.22, 0.30);
static const float WATER_ALPHA = 0.7;

struct WaterVSInput {
  float2 gridPos; // location 0 — (i, j) dans la grille
}

struct WaterVSOutput {
  float4 pos : SV_Position;
  float3 worldPos;
}

[shader("vertex")]
WaterVSOutput waterVertMain(WaterVSInput input) {
  float2 xy = waterPc.origin + input.gridPos * waterPc.cellSize;
  float3 worldPos = float3(xy, waterPc.seaLevel);

  WaterVSOutput output;
  output.pos = mul(ubo.proj, mul(ubo.view, float4(worldPos, 1.0)));
  output.worldPos = worldPos;
  return output;
}

[shader("fragment")]
float4 waterFragMain(WaterVSOutput vertIn) : SV_Target {
  // Surface plane : N = +Z. Même éclairage que le terrain, sans ombre portée.
  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(L.z, 0.0);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 color = WATER_COLOR * (ubo.ambient.rgb + lightCol * NdotL);

  color = applyFog(color, vertIn.worldPos, L, lightCol);
  return float4(color, WATER_ALPHA);
}
