// Literal utility strings keep dynamically rendered gallery classes visible to Tailwind.
export const galleryClasses = {
  card: 'game-card relative aspect-8/3 min-w-0 cursor-pointer overflow-hidden rounded-[10px] border-4 border-transparent bg-[#3c302d] bg-clip-padding hover:border-[var(--popup-bg)] focus-within:border-[var(--popup-bg)] [&.open]:border-[var(--popup-bg)] has-[.game-card-trigger:focus-visible]:outline-2 has-[.game-card-trigger:focus-visible]:outline-offset-3 has-[.game-card-trigger:focus-visible]:outline-cream',
  trigger: 'game-card-trigger absolute inset-0 h-full w-full cursor-pointer select-none border-0 bg-transparent p-0 text-white',
  group: 'game-group not-first:mt-16',
  header: 'game-group-header mb-6 flex items-center gap-4 before:h-px before:flex-1 before:[background:linear-gradient(to_right,transparent,#6a5650)] after:h-px after:flex-1 after:[background:linear-gradient(to_left,transparent,#6a5650)]',
  label: 'game-group-label font-head text-[clamp(1rem,2.4vw,1.25rem)] leading-[1.2] font-bold tracking-[.08em] text-cream uppercase',
  separator: 'game-group-separator mx-[.45rem] font-head text-[1.2rem] leading-none font-bold text-[#6a5650]',
  tagline: 'game-group-tagline font-head text-[clamp(1rem,2.4vw,1.25rem)] leading-[1.2] font-bold text-[#bfaea1]',
  grid: 'game-grid grid grid-cols-4 gap-4 max-[700px]:grid-cols-2 max-[700px]:gap-3 max-[450px]:grid-cols-1',
};

export const artworkClasses = {
  'game-art': 'game-art pointer-events-none absolute inset-0 size-full cursor-pointer object-cover',
  'game-shade': 'game-shade pointer-events-none absolute inset-0 size-full [background:linear-gradient(90deg,rgba(13,7,7,.72),rgba(13,7,7,.18))]',
  'game-logo': 'game-logo pointer-events-none absolute top-[10%] left-[12.5%] h-4/5 w-3/4 cursor-pointer object-contain',
  'game-fallback': 'game-fallback pointer-events-none absolute inset-0 grid place-items-center p-3 text-center font-head text-[clamp(1rem,2vw,1.4rem)] leading-[1.1] font-bold',
};

// In-card links use compact store controls, without SG UI faces or sizing.
export const linkClasses = 'absolute bottom-1 left-1 flex h-6 w-max items-center gap-[5px] rounded-md bg-[rgba(13,7,7,.7)] px-[7px] text-xs leading-none whitespace-nowrap text-[#f3eee7] no-underline [transition:background_.12s_ease,color_.12s_ease,opacity_.15s_ease] hover:bg-white hover:text-[#f51e2b] focus-visible:bg-white focus-visible:text-[#f51e2b] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-cream';
