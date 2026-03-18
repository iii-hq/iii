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
  const lineItem = {
    sku: String(request.body.sku),
    qty: Number(request.body.qty),
  };
  const cart = await iii.trigger({
    function_id: "cart-service::add-item",
    payload: {
      cartId,
      item: lineItem,
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "carts",
      key: cartId,
      value: {
        _key: cartId,
        ...cart,
      },
    },
  });
  logger.info("state.cart_add_item", {
    cartId,
    sku: lineItem.sku,
  });
  return { cart };
});

iii.registerFunction({ id: "carts::get" }, async (request: any) => {
  const logger = new Logger();
  let cart = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "carts",
      key: request.params.cartId,
    },
  });
  if (!cart) {
    cart = await iii.trigger({
      function_id: "cart-service::get-cart",
      payload: {
        cartId: request.params.cartId,
      },
    });
    if (!cart) return { cartId: request.params.cartId, items: [] };
    await iii.trigger({
      function_id: "state::set",
      payload: {
        scope: "carts",
        key: request.params.cartId,
        value: {
          _key: request.params.cartId,
          ...cart,
        },
      },
    });
  }
  logger.info("state.cart_get.found", {
    cartId: request.params.cartId,
    itemCount: cart.items?.length ?? 0,
  });
  return { cart };
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
