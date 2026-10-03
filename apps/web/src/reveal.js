export function createCardReveal() {
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  const animations = new Map();
  const observer = new IntersectionObserver(entries => {
    const visible = entries.filter(entry => entry.isIntersecting)
      .sort((a, b) => a.target.compareDocumentPosition(b.target) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1);
    visible.forEach(({ target: card }, index) => {
      observer.unobserve(card);
      if (!card.classList.contains('reveal-pending')) return;
      if (reducedMotion.matches) {
        show(card);
        return;
      }
      const animation = card.animate([
        { opacity: 0, transform: 'translateY(-14px)' },
        { opacity: 1, transform: 'translateY(0)' },
      ], { duration: 360, delay: index * 80, easing: 'cubic-bezier(.2,.7,.2,1)', fill: 'both' });
      animations.set(card, animation);
      animation.finished.then(() => show(card)).catch(() => {});
    });
  }, { threshold: 0.08 });

  function show(card) {
    observer.unobserve(card);
    animations.get(card)?.cancel();
    animations.delete(card);
    card.classList.remove('reveal-pending');
  }

  reducedMotion.addEventListener('change', () => {
    if (reducedMotion.matches) for (const card of animations.keys()) show(card);
  });

  return {
    observe(card) {
      card.classList.add('reveal-pending');
      observer.observe(card);
    },
    show,
  };
}
