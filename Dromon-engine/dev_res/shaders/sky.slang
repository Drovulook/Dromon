// Ciel : un triangle plein écran au plan far, dessiné après la géométrie opaque.
// La pipeline (test LESS_OR_EQUAL, sans écriture) ne le laisse passer que là où
// rien n'a été dessiné.
#include "lib/atmosphere.slang"

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
  // z = w = 1 → profondeur 1.0, celle du clear.
  output.pos = float4(ndc, 1.0, 1.0);

  // NDC → espace vue : la projection ne fait que multiplier x et y par
  // proj[0][0] et proj[1][1] (le flip Y Vulkan est dans le signe de ce dernier).
  float3 viewRay = float3(ndc.x / ubo.proj[0][0], ndc.y / ubo.proj[1][1], -1.0);
  // Espace vue → monde : inverse de la rotation de `view` = sa transposée.
  output.worldRay = mul(transpose((float3x3)ubo.view), viewRay);
  return output;
}

[shader("fragment")]
float4 skyFragMain(SkyVSOutput vertIn) : SV_Target {
  float3 V = normalize(vertIn.worldRay);
  float3 L = normalize(-ubo.lightDirection.xyz);
  float3 sunCol = ubo.lightColor.rgb * ubo.lightColor.w;

  float3 color = airColor(V, L, sunCol);

  // Disque solaire : V est dans le disque si l'angle (V, L) < rayon, i.e.
  // cos(angle) = dot(V, L) > cos(rayon).
  // Réglages : SunDisk côté Rust (light.rs).
  float radius = ubo.sunDisk.x;
  float disk = smoothstep(cos(radius), cos(radius * ubo.sunDisk.y), dot(V, L));
  color += disk * sunCol * ubo.sunDisk.z;

  return float4(color, 1.0);
}
