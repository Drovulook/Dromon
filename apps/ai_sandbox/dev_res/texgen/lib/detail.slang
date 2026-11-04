// Bruits fractals à octaves tournées : détail fin net, sans trame alignée sur les axes.
#pragma once
#include "procgen/lib/texgen.slang"

// Rotation de 26,6° et fréquence ×√5 par matrice entière : envoie la tuile sur elle-même,
// donc reste périodique. Change de réseau à chaque octave : les nœuds où le bruit de
// gradient s'annule ne s'empilent plus en grille.
float2 nextOctave(float2 q) {
  return frac(float2(2.0 * q.x - q.y, q.x + 2.0 * q.y));
}

// Les trois fBm : `gain` = amplitude d'une octave / précédente. Pentes constantes à
// gain ≈ 0,45 (1/√5) ; au-dessus, le relief fin l'emporte (surface plus « nette »).
// ≈ [−1, 1], en pratique ±0,5.
float fbmR(float2 p, int period, int octaves, float gain, uint seed) {
  float sum = 0.0, amplitude = 1.0, norm = 0.0;
  float2 q = frac(p);
  for (int k = 0; k < octaves; k++) {
    sum += amplitude * gradientNoiseP(q, period, seed + uint(k));
    norm += amplitude;
    amplitude *= gain;
    q = nextOctave(q);
  }
  return sum / norm;
}

// Bosses rondes séparées de plis en V (|bruit|). ≈ [0, 1], moyenne ~0,3.
float billowR(float2 p, int period, int octaves, float gain, uint seed) {
  float sum = 0.0, amplitude = 1.0, norm = 0.0;
  float2 q = frac(p);
  for (int k = 0; k < octaves; k++) {
    sum += amplitude * abs(gradientNoiseP(q, period, seed + uint(k)));
    norm += amplitude;
    amplitude *= gain;
    q = nextOctave(q);
  }
  return sum / norm;
}

// Inverse : arêtes vives, creux arrondis (rugosité minérale). ≈ [0, 1], moyenne ~0,7.
float ridgedR(float2 p, int period, int octaves, float gain, uint seed) {
  return 1.0 - billowR(p, period, octaves, gain, seed);
}

// Courbes de niveau d'un champ `t` en gradins, `ledge` = largeur relative de la marche :
// plateaux aux contours déchiquetés, comme des écailles de pierre. Continu : la marche
// remonte de 1 juste avant que floor() saute.
float ledges(float t, float ledge) {
  return floor(t) + smoothstep(1.0 - ledge, 1.0, frac(t));
}
