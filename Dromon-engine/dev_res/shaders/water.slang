// Surface de la mer (CDLOD) : une grille unique redessinée par nœud, placée par
// push constants, avec geomorphing entre niveaux. Couleur et alpha constants.

#include "lib/atmosphere.slang"

// ⚠ Même disposition que `WaterPushConstants` (world/water/mod.rs).
struct WaterPushConstants {
  float2 origin;     // coin (x min, y min) du nœud, unités monde
  float cellSize;    // côté d'une cellule de la grille
  float seaLevel;
  float morphStart;  // distance caméra → sommet où k quitte 0…
  float morphEnd;    // …et atteint 1 (forme du niveau au-dessus)
  uint level;        // niveau du nœud dans le quadtree (0 = le plus fin)
  uint debugFlags;   // bits WATER_DEBUG_* (debug.ron)
  float testWaveAmplitude; // vagues de test (0 = plat)
  float testWaveLength;
  float maxDistance; // distance horizontale d'affichage (portée du terrain)
};
[[vk::push_constant]]
ConstantBuffer<WaterPushConstants> waterPc;

static const uint WATER_DEBUG_LOD_COLOR = 1;
static const uint WATER_DEBUG_MORPH_COLOR = 2;

static const float3 WATER_COLOR = float3(0.05, 0.22, 0.30);
static const float WATER_ALPHA = 0.7;

// Debug : une couleur par niveau de quadtree (cycle de 6).
static const float3 LOD_COLORS[6] = {
  float3(1.0, 0.2, 0.2), float3(1.0, 0.7, 0.1), float3(0.3, 1.0, 0.3),
  float3(0.2, 0.9, 1.0), float3(0.3, 0.3, 1.0), float3(1.0, 0.3, 1.0),
};

struct WaterVSInput {
  float2 gridPos; // location 0 — (i, j) dans la grille
}

struct WaterVSOutput {
  float4 pos : SV_Position;
  float3 worldPos;
  float morph; // k, pour le debug
}

// Vagues de test : hauteur au point (x, y) du monde.
float testWave(float2 xy) {
  float2 phase = xy * (2.0 * 3.14159265 / waterPc.testWaveLength);
  return waterPc.testWaveAmplitude * sin(phase.x) * sin(phase.y);
}

[shader("vertex")]
WaterVSOutput waterVertMain(WaterVSInput input) {
  float2 grid = input.gridPos;

  // k depuis la position monde AVANT déplacement : deux nœuds voisins calculent le
  // même k sur leur bord commun. Même distance (au plan plat) que le quadtree.
  float3 flatPos = float3(waterPc.origin + grid * waterPc.cellSize, waterPc.seaLevel);
  float dist = distance(flatPos, ubo.cameraPosition.xyz);
  float k = saturate((dist - waterPc.morphStart) / (waterPc.morphEnd - waterPc.morphStart));

  // Sommets impairs (1 sur l'axe concerné) glissés d'une cellule vers leur voisin
  // pair : à k = 1, la grille a la forme exacte de celle du niveau au-dessus.
  float2 odd = frac(grid * 0.5) * 2.0;
  float2 xy = waterPc.origin + (grid - odd * k) * waterPc.cellSize;

  // Hauteur évaluée APRÈS le glissement : un sommet posé sur son voisin en prend la
  // hauteur.
  float3 worldPos = float3(xy, waterPc.seaLevel + testWave(xy));

  WaterVSOutput output;
  output.pos = mul(ubo.proj, mul(ubo.view, float4(worldPos, 1.0)));
  output.worldPos = worldPos;
  output.morph = k;
  return output;
}

[shader("fragment")]
float4 waterFragMain(WaterVSOutput vertIn) : SV_Target {
  // Pas d'eau au-delà du terrain chargé : elle recouvrirait les terres absentes.
  if (distance(vertIn.worldPos.xy, ubo.cameraPosition.xy) > waterPc.maxDistance)
    discard;

  // Normale par face depuis les dérivées d'écran (+Z sur mer plate), tournée vers le
  // haut quel que soit le sens du triangle.
  float3 N = normalize(cross(ddx(vertIn.worldPos), ddy(vertIn.worldPos)));
  if (N.z < 0.0) N = -N;

  float3 albedo = WATER_COLOR;
  if ((waterPc.debugFlags & WATER_DEBUG_LOD_COLOR) != 0)
    albedo = LOD_COLORS[waterPc.level % 6];
  if ((waterPc.debugFlags & WATER_DEBUG_MORPH_COLOR) != 0)
    albedo *= 1.0 - 0.8 * vertIn.morph;

  float3 L = normalize(-ubo.lightDirection.xyz);
  float NdotL = max(dot(N, L), 0.0);
  float3 lightCol = ubo.lightColor.rgb * ubo.lightColor.w;
  float3 color = albedo * (ubo.ambient.rgb + lightCol * NdotL);

  color = applyFog(color, vertIn.worldPos, L, lightCol);
  return float4(color, WATER_ALPHA);
}
