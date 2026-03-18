import { registerWorker, Logger } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "state-iii",
  },
);

iii.registerFunction({ id: "carts::add-item" }, async (request: any) => {
  const logger = new Logger();
  const cartId = request.params.cartId;
  const current = await iii.trigger({
    function_id: "state::get",
    payload: { scope: "carts", key: cartId },
  });
  const cart = current ?? {
    _key: cartId,
    id: cartId,
    items: [],
    checkedOut: false,
  };
  cart.items.push({
    sku: request.body.sku,
    qty: request.body.qty,
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "carts",
      key: cartId,
      value: cart,
    },
  });
  logger.info("state.cart_add_item", {
    cartId,
    sku: request.body.sku,
  });
  return cart;
});

iii.registerFunction({ id: "carts::get" }, async (request: any) => {
  const logger = new Logger();
  const cart = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "carts",
      key: request.params.cartId,
    },
  });
  if (!cart) {
    logger.warn("state.cart_get.not_found", {
      cartId: request.params.cartId,
    });
    const error = new Error("Cart not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  logger.info("state.cart_get.found", {
    cartId: request.params.cartId,
    itemCount: cart.items.length,
  });
  return cart;
});

iii.registerFunction({ id: "carts::checkout" }, async (request: any) => {
  const logger = new Logger();
  const cart = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "carts",
      key: request.params.cartId,
    },
  });
  if (!cart) {
    logger.warn("state.checkout.not_found", {
      cartId: request.params.cartId,
    });
    const error = new Error("Cart not found") as Error & {
      status: number;
    };
    error.status = 404;
    throw error;
  }
  cart.checkedOut = true;
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "carts",
      key: request.params.cartId,
      value: cart,
    },
  });
  logger.info("state.checkout.completed", {
    cartId: request.params.cartId,
  });
  return cart;
});

iii.registerFunction({ id: "carts::on-change" }, async (event: any) => {
  const logger = new Logger();
  const newCart = event.new_value;
  if (newCart?.checkedOut === true && event.old_value?.checkedOut !== true) {
    await iii.trigger({
      function_id: "state::update",
      payload: {
        scope: "metrics",
        key: "global",
        ops: [
          {
            type: "increment",
            path: "checkedout_carts",
            by: 1,
          },
        ],
      },
    });
    logger.info("state.metrics.checkedout_incremented", {
      cartId: newCart.id,
    });
  }
  return { observed: true };
});

iii.registerTrigger({
  type: "state",
  function_id: "carts::on-change",
  config: { scope: "carts" },
});

iii.registerTrigger({
  type: "http",
  function_id: "carts::add-item",
  config: {
    api_path: "/state/carts/:cartId/items",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "carts::get",
  config: {
    api_path: "/state/carts/:cartId",
    http_method: "GET",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "carts::checkout",
  config: {
    api_path: "/state/carts/:cartId/checkout",
    http_method: "POST",
  },
});
